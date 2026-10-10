//! Tackly sync relay: stores encrypted events, relays them to family members
//! by cursor, and pushes new ones over Server-Sent Events.

use std::{collections::HashMap, convert::Infallible, str::FromStr, sync::Arc, time::Duration};

use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{
        IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
    routing::{get, post},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use futures_util::Stream;
use rand::{RngCore, rngs::OsRng};
use sha2::{Digest, Sha256};
use sqlx::{
    Row, SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
};
use tackly_protocol::wire::*;
use tokio::sync::{Mutex, broadcast};
use uuid::Uuid;

#[derive(Clone)]
struct AppState {
    db: SqlitePool,
    /// One wake-up channel per family; carries no data, only "look again".
    live: Arc<Mutex<HashMap<Uuid, broadcast::Sender<()>>>>,
}

impl AppState {
    async fn subscribe(&self, family_id: Uuid) -> broadcast::Receiver<()> {
        let mut live = self.live.lock().await;
        live.entry(family_id)
            .or_insert_with(|| broadcast::channel(64).0)
            .subscribe()
    }

    async fn notify(&self, family_id: Uuid) {
        if let Some(sender) = self.live.lock().await.get(&family_id) {
            let _ = sender.send(());
        }
    }
}

#[derive(Debug)]
struct ApiError(StatusCode, &'static str);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(serde_json::json!({ "error": self.1 }))).into_response()
    }
}

type ApiResult<T> = Result<Json<T>, ApiError>;

fn internal(error: sqlx::Error) -> ApiError {
    eprintln!("database error: {error}");
    ApiError(StatusCode::INTERNAL_SERVER_ERROR, "database error")
}

fn bad_request(message: &'static str) -> ApiError {
    ApiError(StatusCode::BAD_REQUEST, message)
}

fn conflict(message: &'static str) -> ApiError {
    ApiError(StatusCode::CONFLICT, message)
}

fn decode(input: &str, expected: Option<usize>) -> Result<Vec<u8>, ApiError> {
    let bytes = URL_SAFE_NO_PAD
        .decode(input)
        .map_err(|_| bad_request("invalid base64url value"))?;
    if expected.is_some_and(|size| bytes.len() != size) {
        return Err(bad_request("invalid value length"));
    }
    Ok(bytes)
}

fn random_token() -> String {
    let mut bytes = [0_u8; 32];
    OsRng.fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

fn token_hash(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}

struct Device {
    id: Uuid,
    owner: bool,
}

async fn device(db: &SqlitePool, headers: &HeaderMap, family_id: Uuid) -> Result<Device, ApiError> {
    let bearer = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .ok_or(ApiError(StatusCode::UNAUTHORIZED, "device token required"))?;
    let row = sqlx::query(
        "SELECT id, is_owner FROM devices WHERE family_id = ? AND token_hash = ? AND revoked_at IS NULL",
    )
    .bind(family_id.to_string())
    .bind(token_hash(bearer))
    .fetch_optional(db)
    .await
    .map_err(internal)?
    .ok_or(ApiError(StatusCode::UNAUTHORIZED, "invalid device token"))?;
    Ok(Device {
        id: row.get::<String, _>("id").parse().expect("stored UUID"),
        owner: row.get::<i64, _>("is_owner") != 0,
    })
}

async fn create_family(
    State(state): State<AppState>,
    Json(input): Json<CreateFamily>,
) -> ApiResult<DeviceToken> {
    let token = random_token();
    let verifier = decode(&input.recovery_verifier, Some(32))?;
    let mut tx = state.db.begin().await.map_err(internal)?;
    let inserted = sqlx::query("INSERT INTO families (id) VALUES (?) ON CONFLICT DO NOTHING")
        .bind(input.family_id.to_string())
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
    if inserted.rows_affected() == 0 {
        let retried = sqlx::query(
            "UPDATE devices SET token_hash=? WHERE id=? AND family_id=? \
             AND recovery_verifier=? AND is_owner=1 AND revoked_at IS NULL RETURNING id",
        )
        .bind(token_hash(&token))
        .bind(input.device_id.to_string())
        .bind(input.family_id.to_string())
        .bind(verifier)
        .fetch_optional(&mut *tx)
        .await
        .map_err(internal)?;
        if retried.is_none() {
            return Err(conflict("family exists"));
        }
        tx.commit().await.map_err(internal)?;
        return Ok(Json(DeviceToken {
            device_token: token,
            owner: true,
        }));
    }
    sqlx::query(
        "INSERT INTO devices (id, family_id, token_hash, recovery_verifier, is_owner) VALUES (?, ?, ?, ?, 1)",
    )
    .bind(input.device_id.to_string())
    .bind(input.family_id.to_string())
    .bind(token_hash(&token))
    .bind(verifier)
    .execute(&mut *tx)
    .await
    .map_err(internal)?;
    tx.commit().await.map_err(internal)?;
    Ok(Json(DeviceToken {
        device_token: token,
        owner: true,
    }))
}

async fn append_events(
    State(state): State<AppState>,
    Path(family_id): Path<Uuid>,
    headers: HeaderMap,
    Json(input): Json<AppendEvents>,
) -> ApiResult<AppendResult> {
    let caller = device(&state.db, &headers, family_id).await?;
    if input.events.is_empty() || input.events.len() > 8 {
        return Err(bad_request("send between one and eight events"));
    }
    let mut tx = state.db.begin().await.map_err(internal)?;
    // The single SQLite writer connection serializes allocation and commit.
    for event in &input.events {
        if event.origin_device_id != caller.id || event.key_version < 1 {
            return Err(bad_request("invalid event origin or key version"));
        }
        let nonce = decode(&event.nonce, Some(12))?;
        let mac = decode(&event.mac, Some(16))?;
        let ciphertext = decode(&event.ciphertext, None)?;
        if ciphertext.is_empty() || ciphertext.len() > 2 * 1024 * 1024 {
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
            let old = sqlx::query(
                "SELECT aggregate_id, origin_device_id, key_version, nonce, ciphertext, mac \
                 FROM encrypted_events WHERE family_id=? AND event_id=?",
            )
            .bind(family_id.to_string())
            .bind(event.event_id.to_string())
            .fetch_one(&mut *tx)
            .await
            .map_err(internal)?;
            if old.get::<String, _>("aggregate_id") != event.aggregate_id.to_string()
                || old.get::<String, _>("origin_device_id") != event.origin_device_id.to_string()
                || old.get::<i32, _>("key_version") != event.key_version
                || old.get::<Vec<u8>, _>("nonce") != nonce
                || old.get::<Vec<u8>, _>("ciphertext") != ciphertext
                || old.get::<Vec<u8>, _>("mac") != mac
            {
                return Err(conflict("event id already has different ciphertext"));
            }
        }
    }
    tx.commit().await.map_err(internal)?;
    state.notify(family_id).await;
    Ok(Json(AppendResult {
        accepted: input.events.len(),
    }))
}

async fn fetch_events(
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
    Ok(rows
        .into_iter()
        .map(|row| SequencedEvent {
            sequence: row.get("sequence"),
            event: EncryptedEvent {
                event_id: row
                    .get::<String, _>("event_id")
                    .parse()
                    .expect("stored UUID"),
                aggregate_id: row
                    .get::<String, _>("aggregate_id")
                    .parse()
                    .expect("stored UUID"),
                origin_device_id: row
                    .get::<String, _>("origin_device_id")
                    .parse()
                    .expect("stored UUID"),
                key_version: row.get("key_version"),
                nonce: URL_SAFE_NO_PAD.encode(row.get::<Vec<u8>, _>("nonce")),
                ciphertext: URL_SAFE_NO_PAD.encode(row.get::<Vec<u8>, _>("ciphertext")),
                mac: URL_SAFE_NO_PAD.encode(row.get::<Vec<u8>, _>("mac")),
            },
        })
        .collect())
}

async fn list_events(
    State(state): State<AppState>,
    Path(family_id): Path<Uuid>,
    headers: HeaderMap,
    Query(query): Query<EventsQuery>,
) -> ApiResult<EventsPage> {
    device(&state.db, &headers, family_id).await?;
    let after = query.after.unwrap_or(0);
    let limit = query.limit.unwrap_or(MAX_PAGE);
    if after < 0 || !(1..=MAX_PAGE).contains(&limit) {
        return Err(bad_request("invalid cursor or limit"));
    }
    Ok(Json(EventsPage {
        events: fetch_events(&state.db, family_id, after, limit).await?,
    }))
}

/// Server-Sent Events: replays everything after `after` (or `Last-Event-ID`),
/// then pushes each newly accepted event. The SSE `id` is the sequence.
async fn stream_events(
    State(state): State<AppState>,
    Path(family_id): Path<Uuid>,
    headers: HeaderMap,
    Query(query): Query<EventsQuery>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    device(&state.db, &headers, family_id).await?;
    let after = headers
        .get("last-event-id")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse().ok())
        .or(query.after)
        .unwrap_or(0);
    if after < 0 {
        return Err(bad_request("invalid cursor"));
    }
    // Subscribe before the first read so nothing slips between them.
    let wake = state.subscribe(family_id).await;
    let stream = futures_util::stream::unfold(
        (state, wake, after, true),
        move |(state, mut wake, mut cursor, mut first)| async move {
            loop {
                if !first {
                    match wake.recv().await {
                        Ok(()) | Err(broadcast::error::RecvError::Lagged(_)) => {}
                        Err(broadcast::error::RecvError::Closed) => return None,
                    }
                }
                first = false;
                let events = match fetch_events(&state.db, family_id, cursor, MAX_PAGE).await {
                    Ok(events) => events,
                    Err(_) => return None,
                };
                let Some(last) = events.last() else { continue };
                cursor = last.sequence;
                // A full page means there may be more: wake ourselves again.
                if events.len() as i64 == MAX_PAGE {
                    state.notify(family_id).await;
                }
                let message = Event::default()
                    .event(SSE_EVENTS)
                    .id(cursor.to_string())
                    .json_data(EventsPage { events })
                    .expect("serializable page");
                return Some((Ok(message), (state, wake, cursor, false)));
            }
        },
    );
    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15))))
}

async fn create_invite(
    State(state): State<AppState>,
    Path(family_id): Path<Uuid>,
    headers: HeaderMap,
    Json(input): Json<CreateInvite>,
) -> ApiResult<InviteExpiry> {
    let caller = device(&state.db, &headers, family_id).await?;
    if !caller.owner {
        return Err(ApiError(StatusCode::FORBIDDEN, "owner required"));
    }
    let verifier = decode(&input.verifier, Some(32))?;
    let row = sqlx::query(
        "INSERT INTO invitations (id, family_id, verifier_hash, created_by) \
         VALUES (?,?,?,?) RETURNING expires_at",
    )
    .bind(input.invite_id.to_string())
    .bind(family_id.to_string())
    .bind(verifier)
    .bind(caller.id.to_string())
    .fetch_one(&state.db)
    .await
    .map_err(internal)?;
    Ok(Json(InviteExpiry {
        expires_at_utc: row.get("expires_at"),
    }))
}

async fn request_join(
    State(state): State<AppState>,
    Path(invite_id): Path<Uuid>,
    Json(input): Json<InviteProof>,
) -> ApiResult<InviteStatus> {
    let verifier = decode(&input.verifier, Some(32))?;
    let result = sqlx::query(
        "UPDATE invitations SET status='pending', pending_device_id=? \
         WHERE id=? AND verifier_hash=? AND status='open' \
         AND expires_at>strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
    )
    .bind(input.device_id.to_string())
    .bind(invite_id.to_string())
    .bind(verifier)
    .execute(&state.db)
    .await
    .map_err(internal)?;
    if result.rows_affected() != 1 {
        return Err(ApiError(StatusCode::GONE, "invitation unavailable"));
    }
    Ok(Json(InviteStatus {
        status: InviteState::Pending,
        pending_device_id: Some(input.device_id),
    }))
}

async fn invite_status(
    State(state): State<AppState>,
    Path((family_id, invite_id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
) -> ApiResult<InviteStatus> {
    let caller = device(&state.db, &headers, family_id).await?;
    if !caller.owner {
        return Err(ApiError(StatusCode::FORBIDDEN, "owner required"));
    }
    let row = sqlx::query(
        "SELECT status, pending_device_id FROM invitations WHERE id=? AND family_id=? \
         AND expires_at>strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
    )
    .bind(invite_id.to_string())
    .bind(family_id.to_string())
    .fetch_optional(&state.db)
    .await
    .map_err(internal)?
    .ok_or(ApiError(StatusCode::GONE, "invitation unavailable"))?;
    Ok(Json(InviteStatus {
        status: serde_json::from_value(serde_json::Value::String(row.get("status"))).map_err(
            |_| {
                ApiError(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "invalid invitation state",
                )
            },
        )?,
        pending_device_id: row
            .get::<Option<String>, _>("pending_device_id")
            .map(|id| id.parse().expect("stored UUID")),
    }))
}

async fn approve_join(
    State(state): State<AppState>,
    Path((family_id, invite_id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    Json(input): Json<ApproveJoin>,
) -> ApiResult<InviteStatus> {
    let caller = device(&state.db, &headers, family_id).await?;
    if !caller.owner {
        return Err(ApiError(StatusCode::FORBIDDEN, "owner required"));
    }
    let nonce = decode(&input.package_nonce, Some(12))?;
    let ciphertext = decode(&input.package_ciphertext, None)?;
    let mac = decode(&input.package_mac, Some(16))?;
    if ciphertext.is_empty() || ciphertext.len() > 4096 {
        return Err(bad_request("invalid key package size"));
    }
    let result = sqlx::query(
        "UPDATE invitations SET status='approved', package_nonce=?, package_ciphertext=?, package_mac=? \
         WHERE id=? AND family_id=? AND pending_device_id=? AND status='pending' \
         AND expires_at>strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
    )
    .bind(nonce)
    .bind(ciphertext)
    .bind(mac)
    .bind(invite_id.to_string())
    .bind(family_id.to_string())
    .bind(input.device_id.to_string())
    .execute(&state.db)
    .await
    .map_err(internal)?;
    if result.rows_affected() != 1 {
        return Err(ApiError(StatusCode::GONE, "invitation unavailable"));
    }
    Ok(Json(InviteStatus {
        status: InviteState::Approved,
        pending_device_id: Some(input.device_id),
    }))
}

async fn claim_invite(
    State(state): State<AppState>,
    Path(invite_id): Path<Uuid>,
    Json(input): Json<ClaimInvite>,
) -> ApiResult<ClaimedInvite> {
    let verifier = decode(&input.verifier, Some(32))?;
    let recovery_verifier = decode(&input.recovery_verifier, Some(32))?;
    let mut tx = state.db.begin().await.map_err(internal)?;
    let row = sqlx::query(
        "SELECT family_id, package_nonce, package_ciphertext, package_mac FROM invitations \
         WHERE id=? AND verifier_hash=? AND pending_device_id=? \
         AND status='approved' AND expires_at>strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
    )
    .bind(invite_id.to_string())
    .bind(verifier)
    .bind(input.device_id.to_string())
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal)?
    .ok_or(ApiError(StatusCode::GONE, "invitation unavailable"))?;
    let family_id: Uuid = row
        .get::<String, _>("family_id")
        .parse()
        .expect("stored UUID");
    let token = random_token();
    sqlx::query(
        "INSERT INTO devices (id, family_id, token_hash, recovery_verifier) VALUES (?,?,?,?)",
    )
    .bind(input.device_id.to_string())
    .bind(family_id.to_string())
    .bind(token_hash(&token))
    .bind(recovery_verifier)
    .execute(&mut *tx)
    .await
    .map_err(internal)?;
    sqlx::query("UPDATE invitations SET status='used' WHERE id=?")
        .bind(invite_id.to_string())
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
    tx.commit().await.map_err(internal)?;
    Ok(Json(ClaimedInvite {
        family_id,
        device_token: token,
        package_nonce: URL_SAFE_NO_PAD.encode(row.get::<Vec<u8>, _>("package_nonce")),
        package_ciphertext: URL_SAFE_NO_PAD.encode(row.get::<Vec<u8>, _>("package_ciphertext")),
        package_mac: URL_SAFE_NO_PAD.encode(row.get::<Vec<u8>, _>("package_mac")),
    }))
}

async fn cancel_invite(
    State(state): State<AppState>,
    Path((family_id, invite_id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
) -> ApiResult<InviteStatus> {
    let caller = device(&state.db, &headers, family_id).await?;
    if !caller.owner {
        return Err(ApiError(StatusCode::FORBIDDEN, "owner required"));
    }
    sqlx::query(
        "UPDATE invitations SET status='cancelled' WHERE id=? AND family_id=? AND status IN ('open','pending','approved')",
    )
    .bind(invite_id.to_string())
    .bind(family_id.to_string())
    .execute(&state.db)
    .await
    .map_err(internal)?;
    Ok(Json(InviteStatus {
        status: InviteState::Cancelled,
        pending_device_id: None,
    }))
}

/// Opens (creating if needed) the SQLite database with a single writer.
pub async fn connect(database_url: &str) -> Result<SqlitePool, sqlx::Error> {
    let options = SqliteConnectOptions::from_str(database_url)?
        .create_if_missing(true)
        .foreign_keys(true)
        .busy_timeout(Duration::from_secs(10))
        .journal_mode(SqliteJournalMode::Wal);
    SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
}

pub async fn migrate(pool: &SqlitePool) -> Result<(), sqlx::migrate::MigrateError> {
    sqlx::migrate!("./migrations").run(pool).await
}

pub fn router(pool: SqlitePool) -> Router {
    Router::new()
        .route("/health", get(|| async { StatusCode::NO_CONTENT }))
        .route("/v1/families", post(create_family))
        .route(
            "/v1/families/{family_id}/events",
            get(list_events).post(append_events),
        )
        .route("/v1/families/{family_id}/stream", get(stream_events))
        .route("/v1/families/{family_id}/invites", post(create_invite))
        .route(
            "/v1/families/{family_id}/invites/{invite_id}",
            get(invite_status).delete(cancel_invite),
        )
        .route(
            "/v1/families/{family_id}/invites/{invite_id}/approve",
            post(approve_join),
        )
        .route("/v1/invites/{invite_id}/request", post(request_join))
        .route("/v1/invites/{invite_id}/claim", post(claim_invite))
        .layer(DefaultBodyLimit::max(10 * 1024 * 1024))
        .with_state(AppState {
            db: pool,
            live: Arc::default(),
        })
}
