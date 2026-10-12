//! The family's message log: appending, reading by cursor, and following live.
//!
//! Messages are MLS messages to the family's group. The relay cannot read
//! one; it checks who sent it, that it is for the group's current epoch (a
//! commit moves the epoch on; only the head commits), gives it the next
//! sequence number, keeps it forever, and passes it on.

use std::{convert::Infallible, time::Duration};

use axum::{
    Json,
    extract::{Path, Query, State},
    http::HeaderMap,
    response::sse::{Event, KeepAlive, Sse},
};
use futures_util::Stream;
use openmls::prelude::{ContentType, MlsMessageBodyIn, MlsMessageIn, tls_codec::Deserialize as _};
use sqlx::SqlitePool;
use tackly_wire::{
    AppendEvents, AppendResult, Bytes, EncryptedEvent, EventsPage, EventsQuery, MAX_APPEND_BATCH,
    MAX_MESSAGE_BYTES, MAX_PAGE, SSE_EVENTS, STALE_EPOCH, SequencedEvent,
};
use tokio::sync::broadcast::error::RecvError;
use uuid::Uuid;

use crate::{
    AppState,
    auth::Device,
    db::{column, uuid_column},
    error::{ApiError, ApiResult, bad_request, conflict, forbidden, internal},
};

/// What the relay reads of a message: which group, which epoch, what kind.
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
        epoch: epoch.as_u64() as i64,
        content_type,
    })
}

/// Stores up to eight messages from the calling device, all or none. A
/// message for an epoch that is over is refused (the phone catches up and
/// encrypts again); a commit moves the epoch on, and only the head commits.
/// A message ID sent again by the same device is accepted and changes nothing.
pub async fn append_events(
    State(state): State<AppState>,
    Path(family_id): Path<Uuid>,
    headers: HeaderMap,
    Json(input): Json<AppendEvents>,
) -> ApiResult<AppendResult> {
    let caller = Device::authenticate(&state.db, &headers, family_id).await?;
    if caller.agent {
        return Err(forbidden("an AI agent only reads"));
    }
    if input.events.is_empty() || input.events.len() > MAX_APPEND_BATCH {
        return Err(bad_request("send between one and eight events"));
    }
    let mut tx = state.db.begin().await.map_err(internal)?;
    let mut epoch: i64 = {
        let row = sqlx::query("SELECT epoch FROM families WHERE id=?")
            .bind(family_id.to_string())
            .fetch_one(&mut *tx)
            .await
            .map_err(internal)?;
        column(&row, "epoch")?
    };
    for event in &input.events {
        if event.origin_device_id != caller.id {
            return Err(forbidden("a message must come from the sending device"));
        }
        let message = event.message.as_slice().to_vec();
        if message.is_empty() || message.len() > MAX_MESSAGE_BYTES {
            return Err(bad_request("message exceeds limit"));
        }
        let framing = framing(&message, family_id)?;
        if framing.epoch != epoch {
            return Err(conflict(STALE_EPOCH));
        }
        if framing.content_type == ContentType::Commit && !caller.owner {
            return Err(forbidden(
                "only the head of the family changes who is in it",
            ));
        }
        let inserted = sqlx::query(
            "INSERT INTO group_messages \
             (family_id, event_id, origin_device_id, epoch, content_type, message) \
             VALUES (?,?,?,?,?,?) \
             ON CONFLICT (family_id, event_id) DO NOTHING",
        )
        .bind(family_id.to_string())
        .bind(event.event_id.to_string())
        .bind(event.origin_device_id.to_string())
        .bind(epoch)
        .bind(framing.content_type as i64)
        .bind(&message)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
        if inserted.rows_affected() == 0 {
            let stored = sqlx::query(
                "SELECT origin_device_id FROM group_messages WHERE family_id=? AND event_id=?",
            )
            .bind(family_id.to_string())
            .bind(event.event_id.to_string())
            .fetch_one(&mut *tx)
            .await
            .map_err(internal)?;
            if column::<String>(&stored, "origin_device_id")? != event.origin_device_id.to_string()
            {
                return Err(conflict("message id already used by another device"));
            }
            continue;
        }
        if framing.content_type == ContentType::Commit {
            epoch += 1;
            sqlx::query("UPDATE families SET epoch=? WHERE id=?")
                .bind(epoch)
                .bind(family_id.to_string())
                .execute(&mut *tx)
                .await
                .map_err(internal)?;
        }
    }
    tx.commit().await.map_err(internal)?;
    state.live.notify(family_id).await;
    Ok(Json(AppendResult {
        accepted: input.events.len(),
    }))
}

/// Up to `limit` events after the sequence `after`, oldest first.
async fn events_after(
    db: &SqlitePool,
    family_id: Uuid,
    after: i64,
    limit: i64,
) -> Result<Vec<SequencedEvent>, ApiError> {
    let rows = sqlx::query(
        "SELECT sequence, event_id, origin_device_id, message FROM group_messages \
         WHERE family_id=? AND sequence>? ORDER BY sequence LIMIT ?",
    )
    .bind(family_id.to_string())
    .bind(after)
    .bind(limit)
    .fetch_all(db)
    .await
    .map_err(internal)?;
    rows.iter()
        .map(|row| {
            Ok(SequencedEvent {
                sequence: column(row, "sequence")?,
                event: EncryptedEvent {
                    event_id: uuid_column(row, "event_id")?,
                    origin_device_id: uuid_column(row, "origin_device_id")?,
                    message: Bytes::from(column::<Vec<u8>>(row, "message")?),
                },
            })
        })
        .collect()
}

/// One page of history, for a phone catching up.
pub async fn list_events(
    State(state): State<AppState>,
    Path(family_id): Path<Uuid>,
    headers: HeaderMap,
    Query(query): Query<EventsQuery>,
) -> ApiResult<EventsPage> {
    Device::authenticate(&state.db, &headers, family_id).await?;
    let after = query.after.unwrap_or(0);
    let limit = query.limit.unwrap_or(MAX_PAGE);
    if after < 0 || !(1..=MAX_PAGE).contains(&limit) {
        return Err(bad_request("invalid cursor or limit"));
    }
    Ok(Json(EventsPage {
        events: events_after(&state.db, family_id, after, limit).await?,
    }))
}

/// Server-Sent Events: first everything after the cursor (`after`, or the
/// `Last-Event-ID` header of a reconnecting client), then each newly accepted
/// event as it arrives. The SSE `id` is the sequence of the last event sent.
pub async fn stream_events(
    State(state): State<AppState>,
    Path(family_id): Path<Uuid>,
    headers: HeaderMap,
    Query(query): Query<EventsQuery>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    Device::authenticate(&state.db, &headers, family_id).await?;
    let after = headers
        .get("last-event-id")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse().ok())
        .or(query.after)
        .unwrap_or(0);
    if after < 0 {
        return Err(bad_request("invalid cursor"));
    }
    // Subscribe before the first read, so nothing slips in between.
    let wake = state.live.subscribe(family_id).await;
    let stream = futures_util::stream::unfold(
        (state, wake, after, true),
        move |(state, mut wake, mut cursor, mut first_read)| async move {
            loop {
                if !first_read {
                    match wake.recv().await {
                        Ok(()) | Err(RecvError::Lagged(_)) => {}
                        Err(RecvError::Closed) => return None,
                    }
                }
                first_read = false;
                let Ok(events) = events_after(&state.db, family_id, cursor, MAX_PAGE).await else {
                    return None;
                };
                let Some(last) = events.last() else { continue };
                cursor = last.sequence;
                // A full page means there may be more: wake ourselves again.
                if events.len() as i64 == MAX_PAGE {
                    state.live.notify(family_id).await;
                }
                let Ok(message) = Event::default()
                    .event(SSE_EVENTS)
                    .id(cursor.to_string())
                    .json_data(EventsPage { events })
                else {
                    return None;
                };
                return Some((Ok(message), (state, wake, cursor, false)));
            }
        },
    );
    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15))))
}
