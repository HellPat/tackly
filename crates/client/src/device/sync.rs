//! Keeping this phone and the relay in step: uploading what happened here,
//! downloading what happened elsewhere.

use anyhow::Result;
use tackly_protocol::{
    FamilyEvent,
    wire::{EventsPage, MAX_APPEND_BATCH, SequencedEvent},
};
use uuid::Uuid;

use super::{Device, Membership};
use crate::{
    api::Api,
    crypto::{self, SealedData},
};

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

    /// Uploads local events the relay has not acknowledged.
    pub async fn flush(&mut self) -> Result<()> {
        if self.membership.is_none() {
            return Ok(());
        }
        self.ensure_registered().await?;
        let membership = self.joined()?.clone();
        let api = self.api()?;
        let key = self.family_key()?;
        let pending = self.events.lock().pending_events()?;
        for batch in pending.chunks(MAX_APPEND_BATCH) {
            let encrypted = {
                let store = self.events.lock();
                batch
                    .iter()
                    .map(|event| store.outbox_envelope(event, membership.family_id, &key))
                    .collect::<Result<Vec<_>>>()?
            };
            api.append(membership.family_id, &membership.device_token, encrypted)
                .await?;
            let store = self.events.lock();
            for event in batch {
                store.mark_pushed(event.id)?;
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

    /// Decrypts and stores events from the relay. Tampered or undecryptable
    /// events are skipped; the cursor still moves past them.
    pub fn ingest(&mut self, page: EventsPage) -> Result<()> {
        let family_id = self.joined()?.family_id;
        let key = self.family_key()?;
        let store = self.events.lock();
        for update in page.events {
            match open_update(&key, family_id, &update) {
                Some(event) => store.insert(&event, Some(update.sequence))?,
                None => eprintln!(
                    "skipping event {} that failed verification",
                    update.event.event_id
                ),
            }
            store.advance_cursor(update.sequence)?;
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

/// The event inside an update, if it decrypts and its sealed identity matches
/// the routing IDs the relay saw.
fn open_update(key: &[u8], family_id: Uuid, update: &SequencedEvent) -> Option<FamilyEvent> {
    let wire = &update.event;
    let sealed = SealedData {
        nonce: wire.nonce.clone(),
        ciphertext: wire.ciphertext.clone(),
        mac: wire.mac.clone(),
    };
    let plaintext = crypto::open(key, &sealed, &wire.associated_data(family_id)).ok()?;
    let event: FamilyEvent = serde_json::from_slice(&plaintext).ok()?;
    let matches_routing =
        event.id == wire.event_id && event.origin_device_id == wire.origin_device_id;
    matches_routing.then_some(event)
}
