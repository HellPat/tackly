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

/// Keeps the device in sync until `stop` is ready (the app passes one that
/// never is; an agent its Ctrl-C). `on_change` fires after new events were
/// stored, so the UI can re-read the state.
pub async fn run_live_until(
    device: SharedDevice,
    on_change: impl Fn() + Send + Sync + 'static,
    stop: impl Future<Output = ()>,
) {
    run_live_paced(device, on_change, stop, Pace::default()).await;
}

/// How often the live loop does its rounds.
#[derive(Clone, Copy)]
struct Pace {
    /// Looks for expired AI agents (the head's phone takes them out).
    tick: Duration,
    /// Says an idle connection is still there.
    ping: Duration,
    /// Looks again whether the phone is in a family yet.
    idle: Duration,
    /// Waits before connecting again after a drop.
    reconnect: Duration,
}

impl Default for Pace {
    fn default() -> Self {
        Self {
            tick: Duration::from_secs(30),
            ping: Duration::from_secs(30),
            idle: Duration::from_secs(1),
            reconnect: Duration::from_secs(2),
        }
    }
}

async fn run_live_paced(
    device: SharedDevice,
    on_change: impl Fn() + Send + Sync + 'static,
    stop: impl Future<Output = ()>,
    pace: Pace,
) {
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
    let ticker = {
        let device = device.clone();
        async move {
            loop {
                tokio::time::sleep(pace.tick).await;
                let _ = device.lock().await.tick().await;
            }
        }
    };
    tokio::select! {
        () = sync_loop(device, online, on_change, pace) => {}
        () = uploader => {}
        () = ticker => {}
        () = stop => {}
    }
}

async fn sync_loop(
    device: SharedDevice,
    online: Arc<AtomicBool>,
    on_change: impl Fn() + Send + Sync + 'static,
    pace: Pace,
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
            tokio::time::sleep(pace.idle).await;
            continue;
        };
        match api.live(family_id, &token, cursor).await {
            Ok(mut socket) => {
                // Connected, or reconnected after a drop: catch up uploads.
                let _ = device.lock().await.tick().await;
                online.store(true, Ordering::Relaxed);
                on_change();
                let mut ping = tokio::time::interval(pace.ping);
                ping.tick().await;
                loop {
                    let message = tokio::select! {
                        message = socket.next() => message,
                        _ = ping.tick() => {
                            // Keeps the connection open through proxies; the
                            // relay's runtime answers without waking it. A
                            // connection that is gone shows on the next read.
                            let _ = socket.send(Message::text("ping")).await;
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
        tokio::time::sleep(pace.reconnect).await;
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicUsize;

    use anyhow::{Context, Result, ensure};
    use tokio::{net::TcpListener, sync::mpsc};
    use tokio_tungstenite::accept_async;

    use super::*;
    use crate::infrastructure::server_url::ServerUrl;

    const QUICK: Pace = Pace {
        tick: Duration::from_millis(30),
        ping: Duration::from_millis(60),
        idle: Duration::from_millis(20),
        reconnect: Duration::from_millis(20),
    };

    /// What the scripted relay waits for instead of sending.
    const WAIT_FOR_PING: &str = "wait for a ping";
    const WAIT_FOR_GO: &str = "wait for go";

    /// A relay that only speaks the live WebSocket and says what the test
    /// scripted, one list of messages per connection; then the connection
    /// just breaks (no closing handshake). It reports what it heard, and
    /// waits for `go` where the script says so.
    async fn scripted_relay(
        script: Vec<Vec<Message>>,
    ) -> Result<(
        ServerUrl,
        mpsc::UnboundedReceiver<String>,
        mpsc::UnboundedSender<()>,
    )> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let (heard, hearing) = mpsc::unbounded_channel();
        let (go, gone) = mpsc::unbounded_channel::<()>();
        let gone = Arc::new(Mutex::new(gone));
        let script = Arc::new(Mutex::new(script.into_iter()));
        tokio::spawn(async move {
            loop {
                let (stream, _) = listener.accept().await.expect("accept");
                let (script, heard, gone) = (script.clone(), heard.clone(), gone.clone());
                // Each connection on its own: the phone also sends plain HTTP
                // (registering, its backup), which is not a WebSocket.
                tokio::spawn(async move {
                    let Ok(mut socket) = accept_async(stream).await else {
                        return;
                    };
                    for message in script.lock().await.next().unwrap_or_default() {
                        if message == Message::text(WAIT_FOR_PING) {
                            let said = socket.next().await;
                            let _ = heard.send(format!("{said:?}"));
                        } else if message == Message::text(WAIT_FOR_GO) {
                            gone.lock().await.recv().await;
                        } else {
                            let _ = socket.send(message).await;
                        }
                    }
                });
            }
        });
        Ok((ServerUrl::http(addr), hearing, go))
    }

    /// What the relay heard next.
    async fn next(heard: &mut mpsc::UnboundedReceiver<String>) -> Result<String> {
        tokio::time::timeout(Duration::from_secs(10), heard.recv())
            .await
            .context("waiting for the relay")?
            .context("the relay stopped")
    }

    #[tokio::test]
    async fn the_live_loop_skips_noise_pings_reconnects_and_waits_out_of_a_family() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let empty_page = || Message::text(r#"{"events":[]}"#);
        let (relay, mut heard, go) = scripted_relay(vec![
            vec![
                Message::binary(vec![1, 2]),
                Message::text("pong"),
                Message::text("{not json"),
                empty_page(),
                Message::text(WAIT_FOR_PING),
            ],
            vec![
                Message::text(WAIT_FOR_GO),
                empty_page(),
                Message::Close(None),
                // The phone answers the close, then the connection ends.
                Message::text(WAIT_FOR_PING),
            ],
        ])
        .await?;
        let mut phone = Device::open(dir.path())?;
        phone.create_family(&relay, "The Smiths", "Patrick").await?;
        let online = phone.online_flag();
        let device: SharedDevice = Arc::new(Mutex::new(phone));
        let changes = Arc::new(AtomicUsize::new(0));
        let counted = changes.clone();
        let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
        let live = tokio::spawn(run_live_paced(
            device.clone(),
            move || {
                counted.fetch_add(1, Ordering::Relaxed);
            },
            async {
                let _ = stopped.await;
            },
            QUICK,
        ));

        let ping = next(&mut heard).await?;
        ensure!(ping.contains("ping"), "it pings: {ping}");
        ensure!(online.load(Ordering::Relaxed), "the screen shows it online");
        // The first connection broke; on the second, a page comes while the
        // phone is out of the family: it is not taken in.
        tokio::time::sleep(Duration::from_millis(100)).await;
        device.lock().await.logout()?;
        go.send(())?;
        tokio::time::sleep(Duration::from_millis(200)).await;
        ensure!(changes.load(Ordering::Relaxed) >= 4, "the screen was told");
        ensure!(!online.load(Ordering::Relaxed), "offline once it broke");
        let _ = stop.send(());
        tokio::time::timeout(Duration::from_secs(5), live).await??;
        Ok(())
    }

    #[tokio::test]
    async fn the_live_loop_stops_when_told() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let device: SharedDevice = Arc::new(Mutex::new(Device::open(dir.path())?));
        let stop = tokio::time::sleep(Duration::from_millis(50));
        tokio::time::timeout(Duration::from_secs(5), run_live_until(device, || {}, stop)).await?;
        Ok(())
    }
}
