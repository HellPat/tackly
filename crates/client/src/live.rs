//! Keeps a device in sync: uploads pending events, then follows the relay's
//! SSE stream and reconnects with backoff when the network drops.

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

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
    let mut delay = Duration::from_millis(500);
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
        let handle = device.clone();
        let result = api
            .stream(family, &token, cursor, |page| {
                let handle = handle.clone();
                let on_change = &on_change;
                let online = online.clone();
                delay = Duration::from_millis(500);
                async move {
                    let mut device = handle.lock().await;
                    device.ingest(page)?;
                    online.store(true, Ordering::Relaxed);
                    on_change();
                    Ok(())
                }
            })
            .await;
        online.store(false, Ordering::Relaxed);
        on_change();
        if let Err(error) = result {
            eprintln!("live sync: {error}; retrying");
        }
        tokio::time::sleep(delay).await;
        delay = (delay * 2).min(Duration::from_secs(3));
    }
}
