//! Keeping this phone and the relay in step: uploading what happened here,
//! downloading what happened elsewhere. Everything goes through the family's
//! MLS group: an event is an application message; a new member gets the
//! history as a snapshot from the head.

use std::collections::HashSet;

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use tackly_protocol::{
    FamilyEvent,
    wire::{EncryptedEvent, EventsPage, MAX_APPEND_BATCH, MAX_MESSAGE_BYTES},
};
use uuid::Uuid;

use super::{Device, Membership};
use crate::{
    api::{Api, HttpError},
    crypto,
    mls::Incoming,
};

/// What an application message to the group carries.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum Payload {
    Event(FamilyEvent),
    /// The history so far, for a member that just joined. Chunked; only the
    /// head sends it, and only its snapshots are taken.
    Snapshot {
        part: u32,
        parts: u32,
        events: Vec<FamilyEvent>,
    },
}

/// How many times an upload is tried again after the group moved on.
const EPOCH_RETRIES: usize = 3;
/// A snapshot chunk stays well below the relay's limit.
const SNAPSHOT_CHUNK_BYTES: usize = MAX_MESSAGE_BYTES / 2;

/// Everything the live loop needs, so that it does not hold the device lock
/// while it waits on the network.
pub struct LiveTarget {
    pub api: Api,
    pub family_id: Uuid,
    pub token: String,
    /// The last relay sequence this phone has seen.
    pub cursor: i64,
}

impl Device {
    /// Registers a family that was created offline. Nothing to do once done.
    pub async fn ensure_registered(&mut self) -> Result<()> {
        let membership = self.joined()?.clone();
        if membership.registered {
            return Ok(());
        }
        self.api()?
            .create_family(
                membership.family_id,
                self.device_id(),
                &membership.device_token,
            )
            .await?;
        self.save_membership(Membership {
            registered: true,
            ..membership
        })
    }

    /// Encrypts `payload` for the group, as a message from this phone.
    pub(super) fn envelope(&mut self, event_id: Uuid, payload: &Payload) -> Result<EncryptedEvent> {
        let family_id = self.joined()?.family_id;
        let message = self.mls.encrypt(family_id, &serde_json::to_vec(payload)?)?;
        Ok(EncryptedEvent {
            event_id,
            origin_device_id: self.device_id(),
            message: crypto::encode(&message),
        })
    }

    /// Sends messages to the relay. When the group moved on meanwhile (a
    /// stale epoch), it catches up and encrypts them again, a few times.
    pub(super) async fn send(
        &mut self,
        mut make: impl FnMut(&mut Self) -> Result<Vec<EncryptedEvent>>,
    ) -> Result<()> {
        let membership = self.joined()?.clone();
        let api = self.api()?;
        for attempt in 0..=EPOCH_RETRIES {
            let batch = make(self)?;
            match api
                .append(membership.family_id, &membership.device_token, batch)
                .await
            {
                Ok(_) => return Ok(()),
                Err(error) if HttpError::is_stale_epoch(&error) && attempt < EPOCH_RETRIES => {
                    self.pull().await?;
                }
                Err(error) => return Err(error),
            }
        }
        unreachable!("the loop returns")
    }

    /// Sends one commit (a change to who is in the group) to the relay. It is
    /// merged into the group once the relay took it, and dropped otherwise.
    pub(super) async fn commit(
        &mut self,
        mut make: impl FnMut(&mut Self) -> Result<Vec<u8>>,
    ) -> Result<()> {
        let family_id = self.joined()?.family_id;
        let sent = self
            .send(|device| {
                let _ = device.mls.clear_pending(family_id);
                let commit = make(device)?;
                Ok(vec![EncryptedEvent {
                    event_id: Uuid::new_v4(),
                    origin_device_id: device.device_id(),
                    message: crypto::encode(&commit),
                }])
            })
            .await;
        match sent {
            Ok(()) => self.mls.merge_pending(family_id),
            Err(error) => {
                let _ = self.mls.clear_pending(family_id);
                Err(error)
            }
        }
    }

    /// What the live loop does now and then: the head's phone takes expired
    /// AI agents out, then everything pending goes up.
    pub async fn tick(&mut self) -> Result<()> {
        self.expire_agents().await?;
        self.flush().await
    }

    /// Uploads local events the relay has not acknowledged.
    pub async fn flush(&mut self) -> Result<()> {
        let Some(membership) = &self.membership else {
            return Ok(());
        };
        if membership.agent || membership.removed {
            return Ok(());
        }
        self.ensure_registered().await?;
        let pending = self.events.lock().pending_events()?;
        for batch in pending.chunks(MAX_APPEND_BATCH) {
            self.send(|device| {
                batch
                    .iter()
                    .map(|event| device.envelope(event.id, &Payload::Event(event.clone())))
                    .collect()
            })
            .await?;
            let store = self.events.lock();
            for event in batch {
                store.mark_pushed(event.id)?;
            }
        }
        if std::mem::take(&mut self.owes_snapshot) {
            self.send_snapshot().await?;
        }
        self.backup().await
    }

    /// Whether the head came back and still needs the history from this phone.
    pub fn owes_snapshot(&self) -> bool {
        self.owes_snapshot
    }

    /// Sends the whole history to the group, in chunks, for a member that
    /// just joined (only the head does this). Everything the relay has goes
    /// in, so that the new member can skip what came before.
    pub(super) async fn send_snapshot(&mut self) -> Result<()> {
        self.pull().await?;
        let events = self.events.lock().read_events()?;
        let mut chunks: Vec<Vec<FamilyEvent>> = vec![Vec::new()];
        let mut size = 0;
        for event in events {
            let bytes = serde_json::to_vec(&event)?.len();
            if size + bytes > SNAPSHOT_CHUNK_BYTES && !chunks.last().is_some_and(Vec::is_empty) {
                chunks.push(Vec::new());
                size = 0;
            }
            size += bytes;
            if let Some(chunk) = chunks.last_mut() {
                chunk.push(event);
            }
        }
        let parts = chunks.len() as u32;
        for (part, events) in chunks.into_iter().enumerate() {
            let payload = Payload::Snapshot {
                part: part as u32 + 1,
                parts,
                events,
            };
            self.send(|device| Ok(vec![device.envelope(Uuid::new_v4(), &payload)?]))
                .await?;
        }
        Ok(())
    }

    /// Downloads everything after the cursor.
    pub async fn pull(&mut self) -> Result<()> {
        self.ensure_registered().await?;
        let membership = self.joined()?.clone();
        let api = self.api()?;
        loop {
            let after = self.events.lock().cursor()?;
            let page = api
                .events(membership.family_id, &membership.device_token, after)
                .await?;
            if page.events.is_empty() {
                return Ok(());
            }
            self.ingest(page)?;
        }
    }

    /// Takes in messages from the relay. This phone's own come back as
    /// confirmations; the others' are decrypted and checked by the group.
    /// A message that fails is skipped and reported; the cursor still moves.
    pub fn ingest(&mut self, page: EventsPage) -> Result<()> {
        let (family_id, head, me) = (self.joined()?.family_id, self.head()?, self.device_id());
        // AI agents read along; what they send is not taken.
        let agents: HashSet<Uuid> = self
            .state()?
            .agents()
            .map(|agent| agent.device_id)
            .collect();
        for update in page.events {
            let wire = update.event;
            if wire.origin_device_id == me {
                self.events.lock().confirm(wire.event_id, update.sequence)?;
            } else if let Err(error) =
                self.take_in(family_id, head, &agents, &wire, update.sequence)
            {
                eprintln!("skipping message {}: {error:#}", wire.event_id);
            }
            self.events.lock().advance_cursor(update.sequence)?;
        }
        Ok(())
    }

    fn take_in(
        &mut self,
        family_id: Uuid,
        head: Uuid,
        agents: &HashSet<Uuid>,
        wire: &EncryptedEvent,
        sequence: i64,
    ) -> Result<()> {
        let bytes = crypto::decode(&wire.message)?;
        // Only the head changes who is in the family.
        let (sender, bytes) = match self.mls.decrypt(family_id, &bytes, head)? {
            Incoming::Application { sender, bytes } => (sender, bytes),
            Incoming::Commit {
                removed_me: true, ..
            } => {
                let membership = self.joined()?.clone();
                return self.save_membership(Membership {
                    removed: true,
                    ..membership
                });
            }
            Incoming::Commit {
                merged: true,
                from_outside: true,
                ..
            } => {
                // The head lost its phone and is back: it needs the history.
                self.owes_snapshot = true;
                return Ok(());
            }
            Incoming::Commit { .. } | Incoming::Other => return Ok(()),
        };
        ensure!(
            sender == wire.origin_device_id,
            "the sender is not who the relay says"
        );
        ensure!(!agents.contains(&sender), "AI agents only read");
        match serde_json::from_slice::<Payload>(&bytes).context("not a Tackly message")? {
            Payload::Event(event) => {
                ensure!(
                    event.id == wire.event_id && event.origin_device_id == sender,
                    "the event is not what the relay says it is"
                );
                // Before the history is here, an event has nothing to follow.
                if self.events.lock().is_empty()? {
                    self.early.push((event, sequence));
                } else {
                    self.events.lock().insert(&event, Some(sequence))?;
                }
            }
            Payload::Snapshot { events, .. } => {
                let restoring = self.joined()?.restoring;
                ensure!(sender == head || restoring, "only the head sends history");
                let store = self.events.lock();
                for event in &events {
                    // Our own history, if any, keeps its place; the rest comes in order.
                    store.insert(event, Some(sequence))?;
                }
                // What came in the meantime goes after the history.
                for (event, _) in self.early.drain(..) {
                    store.insert(&event, Some(sequence))?;
                }
                drop(store);
                if restoring {
                    let membership = self.joined()?.clone();
                    self.save_membership(Membership {
                        restoring: false,
                        ..membership
                    })?;
                }
            }
        }
        Ok(())
    }

    /// `None` while this phone is not in a family.
    pub fn live_target(&self) -> Option<LiveTarget> {
        let membership = self.membership.as_ref()?;
        Some(LiveTarget {
            api: Api::new(&membership.server_url).ok()?,
            family_id: membership.family_id,
            token: membership.device_token.clone(),
            cursor: self.events.lock().cursor().ok()?,
        })
    }
}
