//! One phone's view of a family: its local event log, membership, and
//! connection to the relay. The family key and device token never leave it.
//!
//! The work is split by what it is about:
//! - [`tasks`]: commands a person gives (add, take, start, pause, finish, lists, their name)
//! - [`places`]: groups of places, places, and which task belongs where
//! - [`pairing`]: inviting someone, and joining
//! - [`sync`]: uploading local events and downloading the others'

mod membership;
mod pairing;
mod places;
mod sync;
mod tasks;

use std::{
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool},
};

use anyhow::{Context, Result, ensure};
use cqrs_es::CqrsFramework;
use tackly_protocol::{CommandContext, Family, FamilyCommand};
use tokio::sync::Notify;
use uuid::Uuid;

pub use membership::Membership;
pub use pairing::{InviteProgress, InviteTicket, JoinRequest};
pub use sync::LiveTarget;

use crate::{
    api::{Api, HttpError},
    cqrs_store::{DeviceStore, UploadTrigger},
    crypto::{self, random_bytes},
    secrets::Secrets,
    store::{EventStore, SharedStore},
};

type Cqrs = CqrsFramework<Family, DeviceStore>;

pub struct Device {
    events: SharedStore,
    cqrs: Cqrs,
    /// Signalled whenever a command committed new local events.
    upload: Arc<Notify>,
    /// True while the live stream to the relay is connected.
    online: Arc<AtomicBool>,
    secrets: Secrets,
    membership: Option<Membership>,
}

impl Device {
    /// Opens (or creates) this phone's data in `data_dir`.
    pub fn open(data_dir: impl Into<PathBuf>) -> Result<Self> {
        let dir: PathBuf = data_dir.into();
        let secrets = Secrets::new(&dir);
        let events = SharedStore::new(EventStore::open(&dir.join("tackly.db"), &secrets)?);
        let membership = Membership::load(&secrets)?;
        let upload = Arc::new(Notify::new());
        Ok(Self {
            cqrs: Self::framework(&events, &upload),
            events,
            upload,
            online: Arc::default(),
            secrets,
            membership,
        })
    }

    fn framework(events: &SharedStore, upload: &Arc<Notify>) -> Cqrs {
        let device_id = events.lock().device_id;
        CqrsFramework::new(
            DeviceStore::new(events.clone()),
            vec![Box::new(UploadTrigger(upload.clone()))],
            CommandContext::for_device(device_id),
        )
    }

    pub fn online_flag(&self) -> Arc<AtomicBool> {
        self.online.clone()
    }

    pub fn upload_signal(&self) -> Arc<Notify> {
        self.upload.clone()
    }

    pub fn device_id(&self) -> Uuid {
        self.events.lock().device_id
    }

    pub fn membership(&self) -> Option<&Membership> {
        self.membership.as_ref()
    }

    /// The family as it stands after replaying every event.
    pub fn state(&self) -> Result<Family> {
        Ok(Family::replay(&self.events.lock().read_events()?))
    }

    // ---- small helpers for the other modules ---------------------------------

    fn joined(&self) -> Result<&Membership> {
        self.membership
            .as_ref()
            .context("this phone is not in a family")
    }

    fn api(&self) -> Result<Api> {
        Api::new(&self.joined()?.server_url)
    }

    fn family_key(&self) -> Result<Vec<u8>> {
        crypto::decode(&self.joined()?.family_key)
    }

    fn save_membership(&mut self, membership: Membership) -> Result<()> {
        membership.save(&self.secrets)?;
        self.membership = Some(membership);
        Ok(())
    }

    /// Runs a command against the family aggregate.
    async fn run(&self, command: FamilyCommand) -> Result<()> {
        let family_id = self.joined()?.family_id;
        self.cqrs.execute(&family_id.to_string(), command).await?;
        Ok(())
    }

    // ---- creating and leaving a family ----------------------------------------

    /// Creates a family with this phone as its head. A relay that cannot be
    /// reached does not stop it: the family works locally and registers on the
    /// next sync. A relay that refuses does.
    pub async fn create_family(
        &mut self,
        server_url: &str,
        family_name: &str,
        my_name: &str,
    ) -> Result<()> {
        ensure!(self.membership.is_none(), "already in a family");
        ensure!(
            !family_name.trim().is_empty() && !my_name.trim().is_empty(),
            "names are required"
        );
        let api = Api::new(server_url)?;
        let family_id = Uuid::new_v4();
        let device_token = crypto::encode(&random_bytes::<32>());
        let registered = match api
            .create_family(family_id, self.device_id(), &device_token)
            .await
        {
            Ok(()) => true,
            Err(error) if error.downcast_ref::<HttpError>().is_some() => return Err(error),
            Err(_) => false,
        };
        self.save_membership(Membership {
            server_url: api.base().to_owned(),
            family_id,
            family_key: crypto::encode(&random_bytes::<32>()),
            device_token,
            registered,
            owner: true,
            name: my_name.trim().to_owned(),
        })?;
        self.run(FamilyCommand::CreateFamily {
            family_id,
            name: family_name.to_owned(),
            owner_name: my_name.to_owned(),
        })
        .await?;
        // Best effort: the sync loop retries when the relay is unreachable.
        let _ = self.flush().await;
        Ok(())
    }

    /// Forgets the family, its events and this phone's identity.
    pub fn logout(&mut self) -> Result<()> {
        self.events.lock().clear(&self.secrets)?;
        self.cqrs = Self::framework(&self.events, &self.upload);
        Membership::forget(&self.secrets)?;
        self.membership = None;
        Ok(())
    }
}
