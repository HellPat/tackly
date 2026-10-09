use std::{env, net::SocketAddr};

use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::{RngCore, rngs::OsRng};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row, postgres::PgPoolOptions};
use uuid::Uuid;

#[derive(Clone)]
struct AppState {
    db: PgPool,
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

async fn device(db: &PgPool, headers: &HeaderMap, family_id: Uuid) -> Result<Device, ApiError> {
    let bearer = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .ok_or(ApiError(StatusCode::UNAUTHORIZED, "device token required"))?;
    let row = sqlx::query(
        "SELECT id, is_owner FROM devices WHERE family_id = $1 AND token_hash = $2 AND revoked_at IS NULL",
    )
    .bind(family_id)
    .bind(token_hash(bearer))
    .fetch_optional(db)
    .await
    .map_err(internal)?
    .ok_or(ApiError(StatusCode::UNAUTHORIZED, "invalid device token"))?;
    Ok(Device {
        id: row.get("id"),
        owner: row.get("is_owner"),
    })
}

#[derive(Deserialize)]
struct CreateFamily {
    family_id: Uuid,
    device_id: Uuid,
    recovery_verifier: String,
}

#[derive(Serialize)]
struct DeviceToken {
    device_token: String,
    owner: bool,
}

async fn create_family(
    State(state): State<AppState>,
    Json(input): Json<CreateFamily>,
) -> ApiResult<DeviceToken> {
    let token = random_token();
    let verifier = decode(&input.recovery_verifier, Some(32))?;
    let mut tx = state.db.begin().await.map_err(internal)?;
    sqlx::query("INSERT INTO families (id) VALUES ($1)")
        .bind(input.family_id)
        .execute(&mut *tx)
        .await
        .map_err(|error| match error {
            sqlx::Error::Database(ref db) if db.is_unique_violation() => conflict("family exists"),
            other => internal(other),
        })?;
    sqlx::query(
        "INSERT INTO devices (id, family_id, token_hash, recovery_verifier, is_owner) VALUES ($1, $2, $3, $4, true)",
    )
    .bind(input.device_id)
    .bind(input.family_id)
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

#[derive(Deserialize)]
struct RecoverDevice {
    device_id: Uuid,
    recovery_verifier: String,
}

async fn recover_device(
    State(state): State<AppState>,
    Path(family_id): Path<Uuid>,
    Json(input): Json<RecoverDevice>,
) -> ApiResult<DeviceToken> {
    let verifier = decode(&input.recovery_verifier, Some(32))?;
    let token = random_token();
    let inserted = sqlx::query(
        "INSERT INTO devices (id, family_id, token_hash, recovery_verifier, is_owner) \
         SELECT $1, family_id, $3, recovery_verifier, is_owner FROM devices \
         WHERE family_id=$2 AND recovery_verifier=$4 AND revoked_at IS NULL \
         LIMIT 1 ON CONFLICT (id) DO NOTHING RETURNING is_owner",
    )
    .bind(input.device_id)
    .bind(family_id)
    .bind(token_hash(&token))
    .bind(verifier)
    .fetch_optional(&state.db)
    .await
    .map_err(internal)?;
    let Some(inserted) = inserted else {
        return Err(ApiError(StatusCode::UNAUTHORIZED, "invalid recovery key"));
    };
    Ok(Json(DeviceToken {
        device_token: token,
        owner: inserted.get("is_owner"),
    }))
}

#[derive(Clone, Deserialize, Serialize)]
struct EncryptedEvent {
    event_id: Uuid,
    aggregate_id: Uuid,
    origin_device_id: Uuid,
    key_version: i32,
    nonce: String,
    ciphertext: String,
    mac: String,
}

#[derive(Deserialize)]
struct AppendEvents {
    events: Vec<EncryptedEvent>,
}

#[derive(Serialize)]
struct AppendResult {
    accepted: usize,
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
    // Allocate sequence numbers only while holding this family's row lock.
    // Otherwise a reader can advance past an uncommitted lower sequence.
    sqlx::query("SELECT id FROM families WHERE id=$1 FOR UPDATE")
        .bind(family_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(internal)?;
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
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8) \
             ON CONFLICT (family_id, event_id) DO NOTHING",
        )
        .bind(family_id)
        .bind(event.event_id)
        .bind(event.aggregate_id)
        .bind(event.origin_device_id)
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
                 FROM encrypted_events WHERE family_id=$1 AND event_id=$2",
            )
            .bind(family_id)
            .bind(event.event_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(internal)?;
            if old.get::<Uuid, _>("aggregate_id") != event.aggregate_id
                || old.get::<Uuid, _>("origin_device_id") != event.origin_device_id
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
    Ok(Json(AppendResult {
        accepted: input.events.len(),
    }))
}

#[derive(Deserialize)]
struct EventsQuery {
    after: Option<i64>,
    limit: Option<i64>,
}

#[derive(Serialize)]
struct SequencedEvent {
    sequence: i64,
    #[serde(flatten)]
    event: EncryptedEvent,
}

#[derive(Serialize)]
struct EventsPage {
    events: Vec<SequencedEvent>,
}

async fn list_events(
    State(state): State<AppState>,
    Path(family_id): Path<Uuid>,
    headers: HeaderMap,
    Query(query): Query<EventsQuery>,
) -> ApiResult<EventsPage> {
    device(&state.db, &headers, family_id).await?;
    let after = query.after.unwrap_or(0);
    let limit = query.limit.unwrap_or(20);
    if after < 0 || !(1..=20).contains(&limit) {
        return Err(bad_request("invalid cursor or limit"));
    }
    let rows = sqlx::query(
        "SELECT sequence, event_id, aggregate_id, origin_device_id, \
         key_version, nonce, ciphertext, mac FROM encrypted_events \
         WHERE family_id=$1 AND sequence>$2 ORDER BY sequence LIMIT $3",
    )
    .bind(family_id)
    .bind(after)
    .bind(limit)
    .fetch_all(&state.db)
    .await
    .map_err(internal)?;
    let events = rows
        .into_iter()
        .map(|row| SequencedEvent {
            sequence: row.get("sequence"),
            event: EncryptedEvent {
                event_id: row.get("event_id"),
                aggregate_id: row.get("aggregate_id"),
                origin_device_id: row.get("origin_device_id"),
                key_version: row.get("key_version"),
                nonce: URL_SAFE_NO_PAD.encode(row.get::<Vec<u8>, _>("nonce")),
                ciphertext: URL_SAFE_NO_PAD.encode(row.get::<Vec<u8>, _>("ciphertext")),
                mac: URL_SAFE_NO_PAD.encode(row.get::<Vec<u8>, _>("mac")),
            },
        })
        .collect();
    Ok(Json(EventsPage { events }))
}

#[derive(Deserialize)]
struct CreateInvite {
    invite_id: Uuid,
    verifier: String,
}

#[derive(Serialize)]
struct InviteExpiry {
    expires_at_utc: String,
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
         VALUES ($1,$2,$3,$4) RETURNING expires_at",
    )
    .bind(input.invite_id)
    .bind(family_id)
    .bind(verifier)
    .bind(caller.id)
    .fetch_one(&state.db)
    .await
    .map_err(internal)?;
    let expiry: time::OffsetDateTime = row.get("expires_at");
    Ok(Json(InviteExpiry {
        expires_at_utc: expiry
            .format(&time::format_description::well_known::Rfc3339)
            .expect("valid expiry"),
    }))
}

#[derive(Deserialize)]
struct InviteProof {
    verifier: String,
    device_id: Uuid,
}

#[derive(Serialize)]
struct InviteStatus {
    status: String,
    pending_device_id: Option<Uuid>,
}

async fn request_join(
    State(state): State<AppState>,
    Path(invite_id): Path<Uuid>,
    Json(input): Json<InviteProof>,
) -> ApiResult<InviteStatus> {
    let verifier = decode(&input.verifier, Some(32))?;
    let result = sqlx::query(
        "UPDATE invitations SET status='pending', pending_device_id=$3 \
         WHERE id=$1 AND verifier_hash=$2 AND status='open' AND expires_at>now()",
    )
    .bind(invite_id)
    .bind(verifier)
    .bind(input.device_id)
    .execute(&state.db)
    .await
    .map_err(internal)?;
    if result.rows_affected() != 1 {
        return Err(ApiError(StatusCode::GONE, "invitation unavailable"));
    }
    Ok(Json(InviteStatus {
        status: "pending".into(),
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
        "SELECT status, pending_device_id FROM invitations WHERE id=$1 AND family_id=$2 AND expires_at>now()",
    )
    .bind(invite_id)
    .bind(family_id)
    .fetch_optional(&state.db)
    .await
    .map_err(internal)?
    .ok_or(ApiError(StatusCode::GONE, "invitation unavailable"))?;
    Ok(Json(InviteStatus {
        status: row.get("status"),
        pending_device_id: row.get("pending_device_id"),
    }))
}

#[derive(Deserialize)]
struct ApproveJoin {
    device_id: Uuid,
    package_nonce: String,
    package_ciphertext: String,
    package_mac: String,
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
        "UPDATE invitations SET status='approved', package_nonce=$4, package_ciphertext=$5, package_mac=$6 \
         WHERE id=$1 AND family_id=$2 AND pending_device_id=$3 AND status='pending' AND expires_at>now()",
    )
    .bind(invite_id)
    .bind(family_id)
    .bind(input.device_id)
    .bind(nonce)
    .bind(ciphertext)
    .bind(mac)
    .execute(&state.db)
    .await
    .map_err(internal)?;
    if result.rows_affected() != 1 {
        return Err(ApiError(StatusCode::GONE, "invitation unavailable"));
    }
    Ok(Json(InviteStatus {
        status: "approved".into(),
        pending_device_id: Some(input.device_id),
    }))
}

#[derive(Deserialize)]
struct ClaimInvite {
    verifier: String,
    device_id: Uuid,
    recovery_verifier: String,
}

#[derive(Serialize)]
struct ClaimedInvite {
    family_id: Uuid,
    device_token: String,
    package_nonce: String,
    package_ciphertext: String,
    package_mac: String,
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
         WHERE id=$1 AND verifier_hash=$2 AND pending_device_id=$3 \
         AND status='approved' AND expires_at>now() FOR UPDATE",
    )
    .bind(invite_id)
    .bind(verifier)
    .bind(input.device_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal)?
    .ok_or(ApiError(StatusCode::GONE, "invitation unavailable"))?;
    let family_id: Uuid = row.get("family_id");
    let token = random_token();
    sqlx::query(
        "INSERT INTO devices (id, family_id, token_hash, recovery_verifier) VALUES ($1,$2,$3,$4)",
    )
    .bind(input.device_id)
    .bind(family_id)
    .bind(token_hash(&token))
    .bind(recovery_verifier)
    .execute(&mut *tx)
    .await
    .map_err(internal)?;
    sqlx::query("UPDATE invitations SET status='used' WHERE id=$1")
        .bind(invite_id)
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
        "UPDATE invitations SET status='cancelled' WHERE id=$1 AND family_id=$2 AND status IN ('open','pending','approved')",
    )
    .bind(invite_id)
    .bind(family_id)
    .execute(&state.db)
    .await
    .map_err(internal)?;
    Ok(Json(InviteStatus {
        status: "cancelled".into(),
        pending_device_id: None,
    }))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let database_url = env::var("DATABASE_URL")?;
    let pool = PgPoolOptions::new()
        .max_connections(10)
        .connect(&database_url)
        .await?;
    if env::args().nth(1).as_deref() == Some("migrate") {
        sqlx::migrate::Migrator::new(std::path::Path::new("./migrations"))
            .await?
            .run(&pool)
            .await?;
        println!("migrations applied");
        return Ok(());
    }
    let app = Router::new()
        .route("/health", get(|| async { StatusCode::NO_CONTENT }))
        .route("/v1/families", post(create_family))
        .route("/v1/families/{family_id}/recover", post(recover_device))
        .route(
            "/v1/families/{family_id}/events",
            get(list_events).post(append_events),
        )
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
        .with_state(AppState { db: pool });
    let address: SocketAddr = env::var("TACKLY_BIND")
        .unwrap_or_else(|_| "127.0.0.1:3000".into())
        .parse()?;
    let listener = tokio::net::TcpListener::bind(address).await?;
    println!("tackly sync listening on {address}");
    axum::serve(listener, app).await?;
    Ok(())
}
