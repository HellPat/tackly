//! The family's message log: appending and reading by cursor.
//!
//! Messages are MLS messages to the family's group. The relay cannot read
//! one; it checks who sent it, that it is for the group's current epoch (a
//! commit moves the epoch on; only the head commits), gives it the next
//! sequence number, keeps it forever, and passes it on.

use std::collections::HashSet;

use openmls::prelude::{ContentType, MlsMessageBodyIn, MlsMessageIn, tls_codec::Deserialize as _};
use serde::Deserialize;
use tackly_wire::{
    AppendEvents, AppendResult, Bytes, EncryptedEvent, EventsPage, MAX_APPEND_BATCH,
    MAX_MESSAGE_BYTES, MAX_PAGE, STALE_EPOCH, SequencedEvent,
};
use uuid::Uuid;

use crate::{
    auth::Device,
    db::{Db, args},
    error::{ApiError, ApiResult, bad_request, conflict, forbidden, internal},
    live,
    relay::{Call, json, stored_uuid},
};

/// What the relay reads of a message: which epoch, what kind.
struct Framing {
    epoch: i64,
    content_type: ContentType,
}

/// The message's public framing, if it is a message to this family's group:
/// a private message (an event, or a commit), or a public commit (the head
/// coming back from outside after losing its phone).
fn framing(bytes: &[u8], family_id: Uuid) -> Result<Framing, ApiError> {
    let message = MlsMessageIn::tls_deserialize_exact(bytes)
        .map_err(|_| bad_request("not an MLS message"))?;
    let (group_id, epoch, content_type) = match message.extract() {
        MlsMessageBodyIn::PrivateMessage(message) => (
            message.group_id().clone(),
            message.epoch(),
            message.content_type(),
        ),
        MlsMessageBodyIn::PublicMessage(message) => (
            message.group_id().clone(),
            message.epoch(),
            message.content_type(),
        ),
        _ => return Err(bad_request("only group messages are relayed")),
    };
    if group_id.as_slice() != family_id.as_bytes() {
        return Err(bad_request("message for another group"));
    }
    if !matches!(content_type, ContentType::Application | ContentType::Commit) {
        return Err(bad_request(
            "only application messages and commits are relayed",
        ));
    }
    Ok(Framing {
        epoch: i64::try_from(epoch.as_u64()).map_err(|_| bad_request("epoch out of range"))?,
        content_type,
    })
}

#[derive(Deserialize)]
struct EpochRow {
    epoch: i64,
}

#[derive(Deserialize)]
struct SenderRow {
    origin_device_id: String,
}

/// Stores up to eight messages from the calling device, all or none. A
/// message for another epoch is refused (the phone catches up and encrypts
/// again); a commit moves the epoch on, and only the head commits. A message
/// ID sent again by the same device is accepted and changes nothing.
pub async fn append(call: &mut Call<'_>, family_id: Uuid) -> ApiResult {
    let caller = Device::authenticate(call, family_id).await?;
    if caller.agent {
        return Err(forbidden("an AI agent only reads"));
    }
    let input: AppendEvents = call.body()?;
    if input.events.is_empty() || input.events.len() > MAX_APPEND_BATCH {
        return Err(bad_request("send between one and eight events"));
    }
    let db = call.db;
    let mut epoch = db
        .first::<EpochRow>("SELECT epoch FROM families WHERE id = ?", args![family_id])
        .await?
        .ok_or_else(|| internal("the caller's family is missing"))?
        .epoch;
    let start_epoch = epoch;
    let mut batch = Vec::new();
    let mut in_batch = HashSet::new();
    for event in &input.events {
        if event.origin_device_id != caller.id {
            return Err(forbidden("a message must come from the sending device"));
        }
        let message = event.message.as_slice();
        if message.is_empty() || message.len() > MAX_MESSAGE_BYTES {
            return Err(bad_request("message exceeds limit"));
        }
        let framing = framing(message, family_id)?;
        if framing.epoch != epoch {
            return Err(conflict(STALE_EPOCH));
        }
        if framing.content_type == ContentType::Commit && !caller.owner {
            return Err(forbidden(
                "only the head of the family changes who is in it",
            ));
        }
        if !in_batch.insert(event.event_id) {
            continue;
        }
        let stored = db
            .first::<SenderRow>(
                "SELECT origin_device_id FROM group_messages WHERE family_id = ? AND event_id = ?",
                args![family_id, event.event_id],
            )
            .await?;
        if let Some(stored) = stored {
            if stored_uuid(&stored.origin_device_id)? != event.origin_device_id {
                return Err(conflict("message id already used by another device"));
            }
            continue;
        }
        batch.push(db.statement(
            "INSERT INTO group_messages \
             (family_id, event_id, origin_device_id, epoch, content_type, message) \
             VALUES (?, ?, ?, ?, ?, ?)",
            args![
                family_id,
                event.event_id,
                event.origin_device_id,
                epoch,
                framing.content_type as i64,
                message,
            ],
        )?);
        if framing.content_type == ContentType::Commit {
            epoch += 1;
        }
    }
    if epoch != start_epoch {
        batch.push(db.statement(
            "UPDATE families SET epoch = ? WHERE id = ?",
            args![epoch, family_id],
        )?);
    }
    let stored_any = !batch.is_empty();
    db.batch(batch).await?;
    if stored_any {
        live::wake(call, family_id).await;
    }
    json(&AppendResult {
        accepted: input.events.len(),
    })
}

/// One page of history, for a phone catching up.
pub async fn list(call: &mut Call<'_>, family_id: Uuid) -> ApiResult {
    Device::authenticate(call, family_id).await?;
    let after = cursor(call.query("after"))?.unwrap_or(0);
    let limit = match call.query("limit") {
        Some(limit) => limit
            .parse()
            .map_err(|_| bad_request("invalid cursor or limit"))?,
        None => MAX_PAGE,
    };
    if !(1..=MAX_PAGE).contains(&limit) {
        return Err(bad_request("invalid cursor or limit"));
    }
    json(&page_after(call.db, family_id, after, limit).await?)
}

/// A cursor from the query: a sequence number, never negative.
pub fn cursor(value: Option<String>) -> Result<Option<i64>, ApiError> {
    let Some(value) = value else {
        return Ok(None);
    };
    match value.parse::<i64>() {
        Ok(after) if after >= 0 => Ok(Some(after)),
        _ => Err(bad_request("invalid cursor or limit")),
    }
}

#[derive(Deserialize)]
struct MessageRow {
    sequence: i64,
    event_id: String,
    origin_device_id: String,
    message: Vec<u8>,
}

/// Up to `limit` messages after the sequence `after`, oldest first.
pub async fn page_after(
    db: &Db,
    family_id: Uuid,
    after: i64,
    limit: i64,
) -> Result<EventsPage, ApiError> {
    let rows: Vec<MessageRow> = db
        .all(
            "SELECT sequence, event_id, origin_device_id, message FROM group_messages \
             WHERE family_id = ? AND sequence > ? ORDER BY sequence LIMIT ?",
            args![family_id, after, limit],
        )
        .await?;
    let events = rows
        .into_iter()
        .map(|row| {
            Ok(SequencedEvent {
                sequence: row.sequence,
                event: EncryptedEvent {
                    event_id: stored_uuid(&row.event_id)?,
                    origin_device_id: stored_uuid(&row.origin_device_id)?,
                    message: Bytes::from(row.message),
                },
            })
        })
        .collect::<Result<_, ApiError>>()?;
    Ok(EventsPage { events })
}
