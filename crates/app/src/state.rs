//! What the screens read and what they can do.
//!
//! [`AppState`] is a small handle made only of signals, so it is `Copy`: any
//! click handler can use it without cloning. The device itself sits behind one
//! lock; every method here takes that lock only for the time of one call.

use std::sync::atomic::Ordering;

use anyhow::Result;
use chrono::{DateTime, Utc};
use dioxus::{core::spawn_forever, prelude::*};
use tackly_client::{Device, Geocoder, InviteTicket, Membership, SharedDevice, run_live};
use tackly_protocol::{Family, GeoPoint, PlaceLocation};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::platform;

/// How long a snackbar stays.
const SNACKBAR_SECONDS: u64 = 5;
/// The screens re-read the device at least this often (the time shown on
/// cards moves on even when nothing happens).
const REFRESH_SECONDS: u64 = 20;

#[derive(Clone, Copy, PartialEq)]
pub struct AppState {
    device: Signal<SharedDevice>,
    pub geocoder: Signal<Geocoder>,
    pub family: Signal<Family>,
    pub membership: Signal<Option<Membership>>,
    /// This phone's own ID, to tell "you" from the others.
    pub my_id: Signal<Uuid>,
    /// Whether the live connection to the relay is up.
    pub online: Signal<bool>,
    pub snackbar: Signal<Option<String>>,
    pub now: Signal<DateTime<Utc>>,
    sync_started: Signal<bool>,
}

impl AppState {
    /// Opens this phone's data. Call once, inside a component.
    pub fn open() -> Result<Self> {
        let device = Device::open(platform::data_dir())?;
        Ok(Self {
            family: Signal::new(device.state()?),
            membership: Signal::new(device.membership().cloned()),
            my_id: Signal::new(device.device_id()),
            device: Signal::new(std::sync::Arc::new(Mutex::new(device))),
            geocoder: Signal::new(Geocoder::from_env()?),
            online: Signal::new(false),
            snackbar: Signal::new(None),
            now: Signal::new(Utc::now()),
            sync_started: Signal::new(false),
        })
    }

    pub fn device(&self) -> SharedDevice {
        self.device.peek().clone()
    }

    /// Reads the device into the signals. Cheap enough to call after any change.
    pub async fn refresh(self) {
        let (family, membership, online, my_id) = {
            let device = self.device().lock_owned().await;
            (
                device.state(),
                device.membership().cloned(),
                device.online_flag().load(Ordering::Relaxed),
                device.device_id(),
            )
        };
        let (mut family_signal, mut membership_signal) = (self.family, self.membership);
        let (mut my_id_signal, mut online_signal, mut now) = (self.my_id, self.online, self.now);
        if let Ok(family) = family
            && *family_signal.peek() != family
        {
            family_signal.set(family);
        }
        membership_signal.set(membership);
        my_id_signal.set(my_id);
        online_signal.set(online);
        now.set(Utc::now());
    }

    /// Shows a snackbar for a few seconds.
    pub fn say(self, message: impl Into<String>) {
        let message = message.into();
        let mut snackbar = self.snackbar;
        snackbar.set(Some(message.clone()));
        spawn_forever(async move {
            tokio::time::sleep(std::time::Duration::from_secs(SNACKBAR_SECONDS)).await;
            if snackbar.peek().as_deref() == Some(message.as_str()) {
                snackbar.set(None);
            }
        });
    }

    /// Runs what a person asked for, then refreshes. A failure becomes a
    /// snackbar. The task belongs to the app, not to the button's component:
    /// sheets close right after the click, and a task owned by a closed
    /// component is cancelled.
    fn act<Fut>(self, action: impl FnOnce(SharedDevice) -> Fut + 'static)
    where
        Fut: std::future::Future<Output = Result<()>> + 'static,
    {
        spawn_forever(async move {
            if let Err(error) = action(self.device()).await {
                self.say(format!("{error:#}"));
            }
            self.refresh().await;
        });
    }

    // ---- what a person can do -------------------------------------------------

    pub fn add_task(self, title: String, emoji: String, place_ids: Vec<Uuid>) {
        self.act(move |device| async move {
            device
                .lock()
                .await
                .add_task(&title, &emoji, &place_ids)
                .await?;
            Ok(())
        });
    }

    pub fn create_place_group(self, name: String, emoji: String) {
        self.act(move |device| async move {
            device
                .lock()
                .await
                .create_place_group(&name, &emoji)
                .await?;
            Ok(())
        });
    }

    pub fn create_place(
        self,
        group_id: Uuid,
        name: String,
        emoji: String,
        location: PlaceLocation,
    ) {
        self.act(move |device| async move {
            device
                .lock()
                .await
                .create_place(group_id, &name, &emoji, location)
                .await?;
            Ok(())
        });
    }

    pub fn add_place_location(self, place_id: Uuid, location: PlaceLocation) {
        self.act(move |device| async move {
            device
                .lock()
                .await
                .add_place_location(place_id, location)
                .await
        });
    }

    /// Puts the task at the place, or takes it away again.
    pub fn set_task_at_place(self, task: Uuid, place: Uuid, here: bool) {
        self.act(move |device| async move {
            let mut device = device.lock().await;
            if here {
                device.add_task_to_place(task, place).await
            } else {
                device.remove_task_from_place(task, place).await
            }
        });
    }

    pub fn start_task(self, task: Uuid) {
        self.act(move |device| async move { device.lock().await.start_task(task).await });
    }

    pub fn complete_task(self, task: Uuid, note: String, location: Option<GeoPoint>) {
        self.act(move |device| async move {
            device
                .lock()
                .await
                .complete_task(task, Some(note), location)
                .await
        });
    }

    pub fn reopen_task(self, task: Uuid) {
        self.act(move |device| async move { device.lock().await.reopen_task(task).await });
    }

    pub fn resolve_conflict(self, task: Uuid, keep_completion: Uuid) {
        self.act(move |device| async move {
            device
                .lock()
                .await
                .resolve_conflict(task, keep_completion)
                .await
        });
    }

    pub fn approve_join(self, ticket: InviteTicket, joining_device: Uuid) {
        self.act(move |device| async move {
            device
                .lock()
                .await
                .approve_join(&ticket, joining_device)
                .await
        });
    }

    pub fn leave_family(self) {
        self.act(move |device| async move { device.lock().await.logout() });
    }

    // ---- live sync -------------------------------------------------------------

    /// Starts keeping this phone in sync, once: whatever arrives from the
    /// relay refreshes the screens.
    pub fn start_sync(self) {
        let mut started = self.sync_started;
        if started.replace(true) {
            return;
        }
        let (changed, mut wake) = tokio::sync::mpsc::unbounded_channel::<()>();
        let device = self.device();
        spawn(async move {
            run_live(device, move || {
                // The receiver lives as long as the app does.
                let _ = changed.send(());
            })
            .await;
        });
        spawn(async move {
            let periodic = std::time::Duration::from_secs(REFRESH_SECONDS);
            loop {
                tokio::select! {
                    update = wake.recv() => if update.is_none() { break },
                    () = tokio::time::sleep(periodic) => {}
                }
                self.refresh().await;
            }
        });
    }
}
