//! Keeps a device in sync: uploads pending events, then follows the relay's
//! live WebSocket and reconnects from its cursor when the network drops.

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use futures_util::{SinkExt, StreamExt};
use tokio::sync::Mutex;
use tokio_tungstenite::tungstenite::Message;

use crate::device::{Device, LiveTarget};

pub type SharedDevice = Arc<Mutex<Device>>;

/// Runs until the task is dropped. `on_change` fires after new events were
/// stored, so the UI can re-read the state.
pub async fn run_live(device: SharedDevice, on_change: impl Fn() + Send + Sync + 'static) {
    let (upload, online) = {
        let device = device.lock().await;
        (device.upload_signal(), device.online_flag())
    };
    let uploader = {
        let device = device.clone();
        async move {
            loop {
                upload.notified().await;
                // Failure is fine: the reconnect loop retries.
                let _ = device.lock().await.flush().await;
            }
        }
    };
    // Now and then: expired AI agents go out (the head's phone does that).
    let ticker = {
        let device = device.clone();
        async move {
            loop {
                tokio::time::sleep(TICK).await;
                let _ = device.lock().await.tick().await;
            }
        }
    };
    tokio::select! {
        () = sync_loop(device, online, on_change) => {}
        () = uploader => {}
        () = ticker => {}
    }
}

/// How often the live loop looks for expired AI agents.
const TICK: Duration = Duration::from_secs(30);
/// How often an idle live connection says it is still there.
const PING: Duration = Duration::from_secs(30);

async fn sync_loop(
    device: SharedDevice,
    online: Arc<AtomicBool>,
    on_change: impl Fn() + Send + Sync + 'static,
) {
    loop {
        let target = {
            let mut device = device.lock().await;
            let _ = device.tick().await;
            device.live_target()
        };
        let Some(LiveTarget {
            api,
            family_id,
            token,
            cursor,
        }) = target
        else {
            tokio::time::sleep(Duration::from_secs(1)).await;
            continue;
        };
        match api.live(family_id, &token, cursor).await {
            Ok(mut socket) => {
                // Connected, or reconnected after a drop: catch up uploads.
                let _ = device.lock().await.tick().await;
                online.store(true, Ordering::Relaxed);
                on_change();
                let mut ping = tokio::time::interval(PING);
                ping.tick().await;
                loop {
                    let message = tokio::select! {
                        message = socket.next() => message,
                        _ = ping.tick() => {
                            // Keeps the connection open through proxies; the
                            // relay's runtime answers without waking it.
                            if socket.send(Message::text("ping")).await.is_err() {
                                break;
                            }
                            continue;
                        }
                    };
                    let text = match message {
                        Some(Ok(Message::Text(text))) => text,
                        Some(Ok(Message::Close(_))) | None => break,
                        Some(Ok(_)) => continue,
                        Some(Err(error)) => {
                            eprintln!("live sync: {error}");
                            break;
                        }
                    };
                    if text.as_str() == "pong" {
                        continue;
                    }
                    match serde_json::from_str(text.as_str()) {
                        Ok(page) => {
                            let mut device = device.lock().await;
                            if let Err(error) = device.ingest(page) {
                                eprintln!("live sync: {error:#}");
                            }
                            // The head came back: it gets the history from here.
                            if device.owes_snapshot() {
                                let _ = device.flush().await;
                            }
                        }
                        Err(error) => eprintln!("live sync: bad update: {error}"),
                    }
                    on_change();
                }
            }
            Err(error) => eprintln!("live sync: {error:#}"),
        }
        online.store(false, Ordering::Relaxed);
        on_change();
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}
