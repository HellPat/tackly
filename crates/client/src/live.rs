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

use crate::device::Device;

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
    tokio::select! {
        () = sync_loop(device, online, on_change) => {}
        () = uploader => {}
    }
}

async fn sync_loop(
    device: SharedDevice,
    online: Arc<AtomicBool>,
    on_change: impl Fn() + Send + Sync + 'static,
) {
    loop {
        let target = {
            let mut device = device.lock().await;
            let _ = device.flush().await;
            device.live_target()
        };
        let Some((api, family, token, cursor)) = target else {
            tokio::time::sleep(Duration::from_secs(1)).await;
            continue;
        };
        match api.event_source(family, &token, cursor) {
            Ok(mut source) => {
                while let Some(event) = source.next().await {
                    match event {
                        // Connected, or reconnected after a drop: catch up uploads.
                        Ok(Event::Open) => {
                            let _ = device.lock().await.flush().await;
                            online.store(true, Ordering::Relaxed);
                        }
                        Ok(Event::Message(message)) => match serde_json::from_str(&message.data) {
                            Ok(page) => {
                                if let Err(error) = device.lock().await.ingest(page) {
                                    eprintln!("live sync: {error:#}");
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
