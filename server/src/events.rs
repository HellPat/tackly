//! The encrypted event log: appending, reading by cursor, and following live.
//!
//! The relay cannot read an event. It checks who sent it, gives it the next
//! sequence number of its family, keeps it forever, and passes it on.

use std::{convert::Infallible, time::Duration};

use axum::{
    Json,
    extract::{Path, Query, State},
    http::HeaderMap,
    response::sse::{Event, KeepAlive, Sse},
};
use futures_util::Stream;
use sqlx::SqlitePool;
use tackly_protocol::wire::{
    AppendEvents, AppendResult, EncryptedEvent, EventsPage, EventsQuery, MAX_APPEND_BATCH,
    MAX_PAGE, SSE_EVENTS, SequencedEvent,
};
use tokio::sync::broadcast::error::RecvError;
use uuid::Uuid;

use crate::{
    AppState,
    auth::Device,
    db::{column, decode, encode, uuid_column},
    error::{ApiError, ApiResult, bad_request, conflict, forbidden, internal},
};

const MAX_CIPHERTEXT_BYTES: usize = 2 * 1024 * 1024;

/// Stores up to eight events from the calling device, all or none. Sending an
/// event again with identical bytes is accepted and changes nothing; the same
/// ID with different bytes is a conflict.
pub async fn append_events(
    State(state): State<AppState>,
    Path(family_id): Path<Uuid>,
    headers: HeaderMap,
    Json(input): Json<AppendEvents>,
) -> ApiResult<AppendResult> {
    let caller = Device::authenticate(&state.db, &headers, family_id).await?;
    if input.events.is_empty() || input.events.len() > MAX_APPEND_BATCH {
        return Err(bad_request("send between one and eight events"));
    }
    let mut tx = state.db.begin().await.map_err(internal)?;
    for event in &input.events {
        if event.origin_device_id != caller.id {
            return Err(forbidden("an event must come from the sending device"));
        }
        if event.key_version < 1 {
            return Err(bad_request("invalid key version"));
        }
        let nonce = decode(&event.nonce, Some(12))?;
        let mac = decode(&event.mac, Some(16))?;
        let ciphertext = decode(&event.ciphertext, None)?;
        if ciphertext.is_empty() || ciphertext.len() > MAX_CIPHERTEXT_BYTES {
            return Err(bad_request("ciphertext exceeds limit"));
        }
        let inserted = sqlx::query(
            "INSERT INTO encrypted_events \
             (family_id, event_id, aggregate_id, origin_device_id, key_version, nonce, ciphertext, mac) \
             VALUES (?,?,?,?,?,?,?,?) \
             ON CONFLICT (family_id, event_id) DO NOTHING",
        )
        .bind(family_id.to_string())
        .bind(event.event_id.to_string())
        .bind(event.aggregate_id.to_string())
        .bind(event.origin_device_id.to_string())
        .bind(event.key_version)
        .bind(&nonce)
        .bind(&ciphertext)
        .bind(&mac)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
        if inserted.rows_affected() == 0 {
            let stored = sqlx::query(
                "SELECT aggregate_id, origin_device_id, key_version, nonce, ciphertext, mac \
                 FROM encrypted_events WHERE family_id=? AND event_id=?",
            )
            .bind(family_id.to_string())
            .bind(event.event_id.to_string())
            .fetch_one(&mut *tx)
            .await
            .map_err(internal)?;
            let identical = column::<String>(&stored, "aggregate_id")?
                == event.aggregate_id.to_string()
                && column::<String>(&stored, "origin_device_id")?
                    == event.origin_device_id.to_string()
                && column::<i32>(&stored, "key_version")? == event.key_version
                && column::<Vec<u8>>(&stored, "nonce")? == nonce
                && column::<Vec<u8>>(&stored, "ciphertext")? == ciphertext
                && column::<Vec<u8>>(&stored, "mac")? == mac;
            if !identical {
                return Err(conflict("event id already has different ciphertext"));
            }
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
        "SELECT sequence, event_id, aggregate_id, origin_device_id, \
         key_version, nonce, ciphertext, mac FROM encrypted_events \
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
                    aggregate_id: uuid_column(row, "aggregate_id")?,
                    origin_device_id: uuid_column(row, "origin_device_id")?,
                    key_version: column(row, "key_version")?,
                    nonce: encode(&column::<Vec<u8>>(row, "nonce")?),
                    ciphertext: encode(&column::<Vec<u8>>(row, "ciphertext")?),
                    mac: encode(&column::<Vec<u8>>(row, "mac")?),
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
