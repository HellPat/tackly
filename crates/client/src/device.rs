//! One phone's view of a family: local event log, membership, pairing, and
//! sync. The family key and device token never leave the device.

use std::path::PathBuf;

use std::sync::{Arc, Mutex, MutexGuard, atomic::AtomicBool};

use anyhow::{Context, Result, ensure};
use cqrs_es::CqrsFramework;
use serde::{Deserialize, Serialize};
use tackly_protocol::{
    Family, FamilyCommand, FamilyEvent, GeoPoint,
    wire::{
        ApproveJoin, ClaimInvite, CreateFamily, CreateInvite, EventsPage, InviteProof, InviteState,
        MAX_APPEND_BATCH,
    },
};
use tokio::sync::Notify;
use uuid::Uuid;

use crate::{
    api::{Api, HttpError},
    cqrs_store::{DeviceStore, UploadTrigger},
    crypto::{self, SealedData, derive, random_bytes},
    secrets::Secrets,
    store::EventStore,
};
use tackly_protocol::Services;

type Cqrs = CqrsFramework<Family, DeviceStore>;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Membership {
    pub server_url: String,
    pub family_id: Uuid,
    pub family_key: String,
    /// Empty until the relay accepted this family (it can be created offline).
    pub device_token: String,
    pub owner: bool,
    pub name: String,
}

impl Membership {
    pub fn registered(&self) -> bool {
        !self.device_token.is_empty()
    }
}

/// What the owner shows the other phone. `code` is pasted or scanned there.
#[derive(Clone, Debug)]
pub struct InviteTicket {
    pub invite_id: Uuid,
    pub code: String,
    secret: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InviteProgress {
    Waiting,
    /// A phone asked to join. Compare `confirmation` with the one it shows.
    Requested {
        device_id: Uuid,
        confirmation: String,
    },
    Approved,
    Gone,
}

/// The joining phone's half-finished request.
#[derive(Clone, Debug)]
pub struct JoinRequest {
    pub confirmation: String,
    invite: InviteLink,
    secret: Vec<u8>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct InviteLink {
    server: String,
    family: Uuid,
    invite: Uuid,
    secret: String,
}

const CODE_PREFIX: &str = "tackly1.";

fn confirmation_code(secret: &[u8], device_id: Uuid) -> String {
    let hash = derive("confirmation", &[secret, device_id.as_bytes()]);
    let number = u32::from_be_bytes([hash[0], hash[1], hash[2], hash[3]]) % 1_000_000;
    format!("{number:06}")
}

fn package_aad(invite_id: Uuid) -> String {
    format!("invite:{invite_id}")
}

pub struct Device {
    events: Arc<Mutex<EventStore>>,
    cqrs: Cqrs,
    upload: Arc<Notify>,
    online: Arc<AtomicBool>,
    secrets: Secrets,
    membership: Option<Membership>,
}

impl Device {
    pub fn open(data_dir: impl Into<PathBuf>) -> Result<Self> {
        let dir: PathBuf = data_dir.into();
        let secrets = Secrets::new(&dir);
        let store = EventStore::open(&dir.join("tackly.db"), &secrets)?;
        let membership = secrets
            .read("membership")?
            .map(|bytes| serde_json::from_slice(&bytes))
            .transpose()
            .context("read family membership")?;
        let events = Arc::new(Mutex::new(store));
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

    fn framework(events: &Arc<Mutex<EventStore>>, upload: &Arc<Notify>) -> Cqrs {
        let device_id = events.lock().unwrap().device_id;
        CqrsFramework::new(
            DeviceStore::new(events.clone()),
            vec![Box::new(UploadTrigger(upload.clone()))],
            Services { device_id },
        )
    }

    fn store(&self) -> MutexGuard<'_, EventStore> {
        self.events.lock().unwrap()
    }

    /// True while the live stream to the relay is connected.
    pub fn online_flag(&self) -> Arc<AtomicBool> {
        self.online.clone()
    }

    /// Signalled whenever a command committed new local events.
    pub fn upload_signal(&self) -> Arc<Notify> {
        self.upload.clone()
    }

    async fn run(&self, command: FamilyCommand) -> Result<()> {
        let family_id = self.joined()?.family_id;
        self.cqrs.execute(&family_id.to_string(), command).await?;
        Ok(())
    }

    pub fn data_dir_default() -> PathBuf {
        std::env::var_os("TACKLY_DATA_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::temp_dir().join("tackly-dev"))
    }

    pub fn device_id(&self) -> Uuid {
        self.store().device_id
    }

    pub fn membership(&self) -> Option<&Membership> {
        self.membership.as_ref()
    }

    pub fn state(&self) -> Result<Family> {
        Ok(Family::replay(&self.store().read_events()?))
    }

    fn save_membership(&mut self, membership: Membership) -> Result<()> {
        self.secrets
            .write("membership", &serde_json::to_vec(&membership)?)?;
        self.membership = Some(membership);
        Ok(())
    }

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

    // ---- family creation and pairing ---------------------------------

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
        let family_key = random_bytes::<32>();
        let recovery_secret = random_bytes::<32>();
        self.secrets.write("recovery_secret", &recovery_secret)?;
        // The relay may be unreachable; the family then works locally and
        // registers on the next sync. A definite refusal still fails.
        let device_token = match api
            .create_family(&CreateFamily {
                family_id,
                device_id: self.device_id(),
                recovery_verifier: crypto::encode(&derive("recovery", &[&recovery_secret])),
            })
            .await
        {
            Ok(token) => token.device_token,
            Err(error) if error.downcast_ref::<HttpError>().is_some() => return Err(error),
            Err(_) => String::new(),
        };
        self.save_membership(Membership {
            server_url: api.base().to_owned(),
            family_id,
            family_key: crypto::encode(&family_key),
            device_token,
            owner: true,
            name: my_name.trim().to_owned(),
        })?;
        self.run(FamilyCommand::CreateFamily {
            family_id,
            list_id: Uuid::new_v4(),
            name: family_name.to_owned(),
            owner_name: my_name.to_owned(),
        })
        .await?;
        let _ = self.flush().await;
        Ok(())
    }

    /// Registers a family that was created offline. No-op once registered.
    pub async fn ensure_registered(&mut self) -> Result<()> {
        let membership = self.joined()?.clone();
        if membership.registered() {
            return Ok(());
        }
        let recovery_secret = self
            .secrets
            .read("recovery_secret")?
            .context("recovery secret is missing")?;
        let token = self
            .api()?
            .create_family(&CreateFamily {
                family_id: membership.family_id,
                device_id: self.device_id(),
                recovery_verifier: crypto::encode(&derive("recovery", &[&recovery_secret])),
            })
            .await?;
        self.save_membership(Membership {
            device_token: token.device_token,
            ..membership
        })
    }

    pub async fn create_invite(&mut self) -> Result<InviteTicket> {
        // The joiner must find the whole history on the relay.
        self.flush().await?;
        let membership = self.joined()?.clone();
        ensure!(membership.owner, "only the owner can invite");
        let secret = random_bytes::<32>().to_vec();
        let invite_id = Uuid::new_v4();
        self.api()?
            .create_invite(
                membership.family_id,
                &membership.device_token,
                &CreateInvite {
                    invite_id,
                    verifier: crypto::encode(&derive("invite-verifier", &[&secret])),
                },
            )
            .await?;
        let link = InviteLink {
            server: membership.server_url,
            family: membership.family_id,
            invite: invite_id,
            secret: crypto::encode(&secret),
        };
        Ok(InviteTicket {
            invite_id,
            code: format!(
                "{CODE_PREFIX}{}",
                crypto::encode(&serde_json::to_vec(&link)?)
            ),
            secret,
        })
    }

    pub async fn invite_progress(&self, ticket: &InviteTicket) -> Result<InviteProgress> {
        let membership = self.joined()?;
        let status = match self
            .api()?
            .invite_status(
                membership.family_id,
                &membership.device_token,
                ticket.invite_id,
            )
            .await
        {
            Ok(status) => status,
            Err(error) if matches!(error.downcast_ref::<HttpError>(), Some(HttpError(code, _)) if code.as_u16() == 410) =>
            {
                return Ok(InviteProgress::Gone);
            }
            Err(error) => return Err(error),
        };
        Ok(match (status.status, status.pending_device_id) {
            (InviteState::Pending, Some(device_id)) => InviteProgress::Requested {
                device_id,
                confirmation: confirmation_code(&ticket.secret, device_id),
            },
            (InviteState::Approved | InviteState::Used, _) => InviteProgress::Approved,
            (InviteState::Cancelled, _) => InviteProgress::Gone,
            _ => InviteProgress::Waiting,
        })
    }

    /// Call only after the person compared the confirmation codes.
    pub async fn approve_join(&self, ticket: &InviteTicket, device_id: Uuid) -> Result<()> {
        let membership = self.joined()?;
        let key = derive("invite-key", &[&ticket.secret]);
        let sealed = crypto::seal(&key, &self.family_key()?, &package_aad(ticket.invite_id))?;
        self.api()?
            .approve_join(
                membership.family_id,
                &membership.device_token,
                ticket.invite_id,
                &ApproveJoin {
                    device_id,
                    package_nonce: sealed.nonce,
                    package_ciphertext: sealed.ciphertext,
                    package_mac: sealed.mac,
                },
            )
            .await?;
        Ok(())
    }

    /// Joining phone, step 1: ask the owner to let this phone in.
    pub async fn request_join(&mut self, code: &str) -> Result<JoinRequest> {
        ensure!(self.membership.is_none(), "already in a family");
        let encoded = code
            .trim()
            .strip_prefix(CODE_PREFIX)
            .context("not a Tackly invitation code")?;
        let link: InviteLink = serde_json::from_slice(&crypto::decode(encoded)?)
            .context("invitation code is damaged")?;
        let secret = crypto::decode(&link.secret)?;
        Api::new(&link.server)?
            .request_join(
                link.invite,
                &InviteProof {
                    verifier: crypto::encode(&derive("invite-verifier", &[&secret])),
                    device_id: self.device_id(),
                },
            )
            .await
            .context("the invitation is expired or already used")?;
        Ok(JoinRequest {
            confirmation: confirmation_code(&secret, self.device_id()),
            invite: link,
            secret,
        })
    }

    /// Joining phone, step 2: poll until the owner approved. `Ok(false)` means
    /// not approved yet.
    pub async fn complete_join(&mut self, request: &JoinRequest, my_name: &str) -> Result<bool> {
        ensure!(!my_name.trim().is_empty(), "name is required");
        let api = Api::new(&request.invite.server)?;
        let recovery_secret = random_bytes::<32>();
        let claimed = match api
            .claim_invite(
                request.invite.invite,
                &ClaimInvite {
                    verifier: crypto::encode(&derive("invite-verifier", &[&request.secret])),
                    device_id: self.device_id(),
                    recovery_verifier: crypto::encode(&derive("recovery", &[&recovery_secret])),
                },
            )
            .await
        {
            Ok(claimed) => claimed,
            Err(error) if matches!(error.downcast_ref::<HttpError>(), Some(HttpError(code, _)) if code.as_u16() == 410) =>
            {
                return Ok(false);
            }
            Err(error) => return Err(error),
        };
        ensure!(
            claimed.family_id == request.invite.family,
            "invitation belongs to another family"
        );
        let family_key = crypto::open(
            &derive("invite-key", &[&request.secret]),
            &SealedData {
                nonce: claimed.package_nonce,
                ciphertext: claimed.package_ciphertext,
                mac: claimed.package_mac,
            },
            &package_aad(request.invite.invite),
        )?;
        self.secrets.write("recovery_secret", &recovery_secret)?;
        self.save_membership(Membership {
            server_url: api.base().to_owned(),
            family_id: claimed.family_id,
            family_key: crypto::encode(&family_key),
            device_token: claimed.device_token,
            owner: false,
            name: my_name.trim().to_owned(),
        })?;
        self.pull().await?;
        self.run(FamilyCommand::Join {
            name: my_name.to_owned(),
        })
        .await?;
        self.flush().await?;
        Ok(true)
    }

    pub fn logout(&mut self) -> Result<()> {
        self.store().clear(&self.secrets)?;
        self.cqrs = Self::framework(&self.events, &self.upload);
        self.secrets.delete("membership")?;
        self.secrets.delete("recovery_secret")?;
        self.membership = None;
        Ok(())
    }

    // ---- tasks --------------------------------------------------------

    pub fn default_list(&self) -> Result<Uuid> {
        self.state()?
            .lists
            .keys()
            .next()
            .copied()
            .context("the family has no list yet")
    }

    pub async fn add_task(&mut self, title: &str, emoji: &str) -> Result<Uuid> {
        let task_id = Uuid::new_v4();
        self.run(FamilyCommand::AddTask {
            task_id,
            list_id: self.default_list()?,
            title: title.to_owned(),
            emoji: emoji.to_owned(),
        })
        .await?;
        Ok(task_id)
    }

    pub async fn start_task(&mut self, task_id: Uuid) -> Result<()> {
        self.run(FamilyCommand::StartTask { task_id }).await
    }

    /// Duration and the start reference come from the replayed
    /// `task.started`; the caller supplies note and location.
    pub async fn complete_task(
        &mut self,
        task_id: Uuid,
        note: Option<String>,
        location: Option<GeoPoint>,
    ) -> Result<()> {
        self.run(FamilyCommand::CompleteTask {
            task_id,
            note,
            location,
        })
        .await
    }

    pub async fn reopen_task(&mut self, task_id: Uuid) -> Result<()> {
        self.run(FamilyCommand::ReopenTask { task_id }).await
    }

    /// Any member who finished the task may settle a double completion.
    pub async fn resolve_conflict(&mut self, task_id: Uuid, keep: Uuid) -> Result<()> {
        self.run(FamilyCommand::ResolveConflict {
            task_id,
            keep_completion_event_id: keep,
        })
        .await
    }

    // ---- sync ---------------------------------------------------------

    /// Uploads local events the server has not acknowledged.
    pub async fn flush(&mut self) -> Result<()> {
        if self.membership.is_none() {
            return Ok(());
        }
        self.ensure_registered().await?;
        let membership = self.joined()?.clone();
        let api = self.api()?;
        let key = self.family_key()?;
        let pending = self.store().pending_events()?;
        for batch in pending.chunks(MAX_APPEND_BATCH) {
            let wire = batch
                .iter()
                .map(|event| {
                    self.store()
                        .outbox_envelope(event, membership.family_id, &key)
                })
                .collect::<Result<Vec<_>>>()?;
            api.append(membership.family_id, &membership.device_token, wire)
                .await?;
            for event in batch {
                self.store().mark_pushed(event.id)?;
            }
        }
        Ok(())
    }

    /// Downloads everything after the cursor.
    pub async fn pull(&mut self) -> Result<()> {
        self.ensure_registered().await?;
        let membership = self.joined()?.clone();
        let api = self.api()?;
        loop {
            let after = self.store().cursor()?;
            let page = api
                .events(membership.family_id, &membership.device_token, after)
                .await?;
            if page.events.is_empty() {
                return Ok(());
            }
            self.ingest(page)?;
        }
    }

    /// Decrypts and stores events from the relay. Tampered or undecryptable
    /// events are skipped; the cursor still advances past them.
    pub fn ingest(&mut self, page: EventsPage) -> Result<()> {
        let family_id = self.joined()?.family_id;
        let key = self.family_key()?;
        for sequenced in page.events {
            let wire = &sequenced.event;
            let opened = crypto::open(
                &key,
                &SealedData {
                    nonce: wire.nonce.clone(),
                    ciphertext: wire.ciphertext.clone(),
                    mac: wire.mac.clone(),
                },
                &wire.associated_data(family_id),
            )
            .ok()
            .and_then(|bytes| serde_json::from_slice::<FamilyEvent>(&bytes).ok())
            .filter(|event| {
                event.id == wire.event_id
                    && event.subject_id == wire.aggregate_id
                    && event.origin_device_id == wire.origin_device_id
            });
            match opened {
                Some(event) => self.store().insert(&event, Some(sequenced.sequence))?,
                None => eprintln!("skipping event {} that failed verification", wire.event_id),
            }
            self.store().advance_cursor(sequenced.sequence)?;
        }
        Ok(())
    }

    /// Everything the live loop needs, so it need not hold the device lock
    /// while waiting on the network.
    pub fn live_target(&self) -> Option<(Api, Uuid, String, i64)> {
        let membership = self.membership.as_ref()?;
        Some((
            Api::new(&membership.server_url).ok()?,
            membership.family_id,
            membership.device_token.clone(),
            self.store().cursor().ok()?,
        ))
    }
}
