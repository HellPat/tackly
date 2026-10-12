//! One phone's view of a family: its local event log, membership, and
//! connection to the relay. Its MLS keys and device token never leave it.
//!
//! The work is split by what it is about:
//! - [`tasks`]: commands a person gives (add, take, start, pause, finish, lists, their name)
//! - [`places`]: groups of places, places, and which task belongs where
//! - [`problems`]: problems with their solutions, and each person's plan for a day
//! - [`pairing`]: inviting someone, and joining
//! - [`agent_access`]: an AI agent asks for access, the head allows it
//! - [`recovery`]: the head's recovery code and the way back after a lost phone
//! - [`sync`]: uploading local events and downloading the others'
//!
//! The family is an MLS group ([`crate::infrastructure::group_crypto`]): every event goes to the relay
//! as a message only the group's members can read.

mod agent_access;
mod invite_link;
mod membership;
mod pairing;
mod places;
mod problems;
mod recovery;
mod recovery_code;
mod sync;
mod tasks;

use std::{
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool},
};

use anyhow::{Context, Result, ensure};
use cqrs_es::CqrsFramework;
use tackly_domain::{CommandContext, Family, FamilyCommand, Name};
use tokio::sync::Notify;
use uuid::Uuid;

pub use agent_access::{AGENT_NAME, AgentAccess, AgentAsk};
pub use invite_link::{InviteLink, InviteLinkError};
pub use membership::Membership;
pub use pairing::{InviteProgress, InviteTicket, JoinRequest};
pub use recovery_code::{RecoveryCode, RecoveryCodeError};
pub use sync::LiveTarget;

use crate::{
    infrastructure::cqrs_store::{DeviceStore, UploadTrigger},
    infrastructure::device_token::DeviceToken,
    infrastructure::group_crypto::Mls,
    infrastructure::local_store::{EventStore, SharedStore},
    infrastructure::relay_client::{Api, HttpError},
    infrastructure::secrets::Secrets,
    infrastructure::server_url::ServerUrl,
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
    mls: Mls,
    /// Events that arrived while this phone still waits for the head's
    /// history snapshot: they belong after it.
    early: Vec<(tackly_domain::FamilyEvent, i64)>,
    /// What the last recovery backup covered (events, epoch).
    backed_up: Option<(usize, u64)>,
    /// The head came back from outside: it needs the history from us.
    owes_snapshot: bool,
    /// The relay restarted and may have lost some of our events: send them again.
    owes_resend: bool,
}

impl Device {
    /// Opens (or creates) this phone's data in `data_dir`.
    pub fn open(data_dir: impl Into<PathBuf>) -> Result<Self> {
        let dir: PathBuf = data_dir.into();
        let secrets = Secrets::new(&dir);
        let events = SharedStore::new(EventStore::open(&dir.join("tackly.db"), &secrets)?);
        let membership = Membership::load(&secrets)?;
        let upload = Arc::new(Notify::new());
        let mls = Mls::open(&secrets, events.lock().device_id)?;
        Ok(Self {
            cqrs: Self::framework(&events, &upload),
            events,
            upload,
            online: Arc::default(),
            secrets,
            membership,
            mls,
            early: Vec::new(),
            backed_up: None,
            owes_snapshot: false,
            owes_resend: false,
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

    /// Every event this phone has, in the order it replays them.
    pub fn events(&self) -> Result<Vec<tackly_domain::FamilyEvent>> {
        self.events.lock().read_events()
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

    /// The head of the family (this phone, or whoever let it in).
    fn head(&self) -> Result<Uuid> {
        Ok(self.joined()?.head)
    }

    fn save_membership(&mut self, membership: Membership) -> Result<()> {
        membership.save(&self.secrets)?;
        self.membership = Some(membership);
        Ok(())
    }

    /// Runs a command against the family aggregate.
    async fn run(&self, command: FamilyCommand) -> Result<()> {
        let membership = self.joined()?;
        ensure!(!membership.agent, "an AI agent only reads");
        let family_id = membership.family_id;
        self.cqrs.execute(&family_id.to_string(), command).await?;
        Ok(())
    }

    // ---- creating and leaving a family ----------------------------------------

    /// Creates a family with this phone as its head. A relay that cannot be
    /// reached does not stop it: the family works locally and registers on the
    /// next sync. A relay that refuses does.
    pub async fn create_family(
        &mut self,
        server_url: &ServerUrl,
        family_name: &str,
        my_name: &str,
    ) -> Result<()> {
        ensure!(self.membership.is_none(), "already in a family");
        let (family_name, my_name): (Name, Name) = (family_name.parse()?, my_name.parse()?);
        let api = Api::new(server_url)?;
        let family_id = Uuid::new_v4();
        let device_token = DeviceToken::generate();
        let registered = match api
            .create_family(family_id, self.device_id(), &device_token)
            .await
        {
            Ok(()) => true,
            Err(error) if error.downcast_ref::<HttpError>().is_some() => return Err(error),
            Err(_) => false,
        };
        self.mls.create_group(family_id)?;
        self.save_membership(Membership {
            server_url: api.base().clone(),
            family_id,
            head: self.device_id(),
            device_token,
            registered,
            owner: true,
            name: my_name.clone(),
            agent: false,
            removed: false,
            restoring: false,
        })?;
        self.run(FamilyCommand::CreateFamily {
            family_id,
            name: family_name,
            owner_name: my_name,
        })
        .await?;
        // The recovery code comes with the family; the person must save it.
        self.ensure_recovery_code().await?;
        // Best effort: the sync loop retries when the relay is unreachable.
        let _ = self.flush().await;
        Ok(())
    }

    /// Takes a member (or an AI agent) out of the family: the group moves to
    /// a key that phone never gets, the relay stops serving it, and the
    /// family's record says so.
    pub async fn remove_member(&mut self, member_id: Uuid) -> Result<()> {
        let membership = self.joined()?.clone();
        ensure!(
            membership.owner,
            "only the head of the family can remove someone"
        );
        ensure!(member_id != self.device_id(), "the head cannot be removed");
        self.flush().await?;
        if self.mls.members(membership.family_id)?.contains(&member_id) {
            self.commit(|device| device.mls.remove_member(membership.family_id, member_id))
                .await?;
        }
        self.api()?
            .revoke_device(membership.family_id, &membership.device_token, member_id)
            .await?;
        self.run(FamilyCommand::RemoveMember { member_id }).await?;
        self.flush().await
    }

    /// The head's phone takes AI agents out whose time is over (MLS cannot
    /// schedule a removal, so the next phone that can, does it).
    pub(super) async fn expire_agents(&mut self) -> Result<()> {
        if !self.membership.as_ref().is_some_and(|m| m.owner) {
            return Ok(());
        }
        let now = chrono::Utc::now();
        let expired: Vec<Uuid> = self
            .state()?
            .agents()
            .filter(|agent| agent.agent_until.is_some_and(|until| until <= now))
            .map(|agent| agent.device_id)
            .collect();
        for agent in expired {
            self.remove_member(agent).await?;
        }
        Ok(())
    }

    /// Forgets the family, its events and this phone's identity.
    pub fn logout(&mut self) -> Result<()> {
        self.events.lock().clear(&self.secrets)?;
        self.mls.clear(self.events.lock().device_id)?;
        self.cqrs = Self::framework(&self.events, &self.upload);
        Membership::forget(&self.secrets)?;
        for name in [
            "recovery-secret",
            "recovery-recipient",
            // Older versions.
            "recovery-words",
            "recovery-confirmed",
        ] {
            self.secrets.delete(name)?;
        }
        self.membership = None;
        self.backed_up = None;
        self.owes_snapshot = false;
        self.owes_resend = false;
        Ok(())
    }
}
