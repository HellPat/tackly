use std::sync::{Arc, atomic::Ordering};

use anyhow::Result;
use dioxus::{core::spawn_forever, prelude::*};
use tackly_client::{Device, Membership, SharedDevice, run_live};
use tackly_protocol::Family;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::platform;

/// Everything screens read, as signals, plus the device they act through.
#[derive(Clone)]
pub struct AppState {
    pub device: SharedDevice,
    pub family: Signal<Family>,
    pub membership: Signal<Option<Membership>>,
    pub my_id: Signal<Uuid>,
    pub online: Signal<bool>,
    pub toast: Signal<Option<String>>,
    pub now: Signal<chrono::DateTime<chrono::Utc>>,
    started: Signal<bool>,
}

impl AppState {
    pub fn open() -> Result<Self> {
        let device = Device::open(platform::data_dir())?;
        let family = device.state()?;
        let membership = device.membership().cloned();
        let my_id = device.device_id();
        Ok(Self {
            device: Arc::new(Mutex::new(device)),
            family: Signal::new(family),
            membership: Signal::new(membership),
            my_id: Signal::new(my_id),
            online: Signal::new(false),
            toast: Signal::new(None),
            now: Signal::new(chrono::Utc::now()),
            started: Signal::new(false),
        })
    }

    /// Reads the device into the signals. Cheap enough to call after any change.
    pub async fn refresh(mut self) {
        let device = self.device.lock().await;
        if let Ok(family) = device.state() {
            if *self.family.peek() != family {
                self.family.set(family);
            }
        }
        let membership = device.membership().cloned();
        let online = device.online_flag().load(Ordering::Relaxed);
        let my_id = device.device_id();
        drop(device);
        self.membership.set(membership);
        self.my_id.set(my_id);
        self.online.set(online);
        self.now.set(chrono::Utc::now());
    }

    /// Shows a snackbar for a few seconds.
    pub fn say(mut self, message: impl Into<String>) {
        let message = message.into();
        self.toast.set(Some(message.clone()));
        let mut toast = self.toast;
        spawn_forever(async move {
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            if toast.peek().as_deref() == Some(message.as_str()) {
                toast.set(None);
            }
        });
    }

    /// Runs a user action, then refreshes. Errors become a snackbar. The task
    /// belongs to the app, not to the button's component: sheets close right
    /// after the click, and a task owned by a closed component is cancelled.
    pub fn run<T: 'static>(self, action: impl std::future::Future<Output = Result<T>> + 'static) {
        spawn_forever(async move {
            if let Err(error) = action.await {
                self.clone().say(format!("{error:#}"));
            }
            self.refresh().await;
        });
    }

    /// Starts live sync once: events from the relay refresh the screens.
    pub fn start_sync(&self) {
        if *self.started.peek() {
            return;
        }
        let mut started = self.started;
        started.set(true);
        let (changed, mut wake) = tokio::sync::mpsc::unbounded_channel::<()>();
        let device = self.device.clone();
        spawn(async move {
            run_live(device, move || {
                let _ = changed.send(());
            })
            .await;
        });
        let state = self.clone();
        spawn(async move {
            loop {
                tokio::select! {
                    got = wake.recv() => if got.is_none() { break },
                    () = tokio::time::sleep(std::time::Duration::from_secs(20)) => {}
                }
                state.clone().refresh().await;
            }
        });
    }
}
