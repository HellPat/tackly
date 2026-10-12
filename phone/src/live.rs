//! Keeps a device in sync: uploads pending events, then follows the relay's
//! SSE stream and reconnects with backoff when the network drops.

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use futures_util::StreamExt;
use reqwest_eventsource::Event;
use tokio::sync::Mutex;

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
        match api.event_source(family_id, &token, cursor) {
            Ok(mut source) => {
                while let Some(event) = source.next().await {
                    match event {
                        // Connected, or reconnected after a drop: catch up uploads.
                        Ok(Event::Open) => {
                            let _ = device.lock().await.tick().await;
                            online.store(true, Ordering::Relaxed);
                        }
                        Ok(Event::Message(message)) => match serde_json::from_str(&message.data) {
                            Ok(page) => {
                                let mut device = device.lock().await;
                                if let Err(error) = device.ingest(page) {
                                    eprintln!("live sync: {error:#}");
                                }
                                // The head came back and gets the history from
                                // here, or the relay restarted and may miss ours.
                                if device.owes_upload() {
                                    let _ = device.flush().await;
                                }
                            }
                            Err(error) => eprintln!("live sync: bad update: {error}"),
                        },
                        // The source retries by itself unless it closed.
                        Err(error) => {
                            online.store(false, Ordering::Relaxed);
                            eprintln!("live sync: {error}");
                        }
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
