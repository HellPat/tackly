//! Letting a new phone join a family.
//!
//! An invitation goes open → pending (a phone asked, with its MLS key
//! package) → approved (the head added it to the group and left the welcome
//! here) → used. The relay only moves key package and welcome along; the
//! secret both phones derive their digits from never reaches it.

use axum::{
    Json,
    extract::{Path, State},
    http::HeaderMap,
};
use tackly_protocol::wire::{
    ApproveJoin, ClaimInvite, ClaimedInvite, CreateInvite, InviteExpiry, InviteProof, InviteState,
    InviteStatus,
};
use uuid::Uuid;

use crate::{
    AppState,
    auth::Device,
    db::{column, decode, encode, uuid_column},
    error::{ApiError, ApiResult, bad_request, gone, internal},
};

const MAX_KEY_PACKAGE_BYTES: usize = 8 * 1024;
/// A welcome carries the group's tree: it grows with the family.
const MAX_WELCOME_BYTES: usize = 256 * 1024;

/// True while the invitation's five minutes have not passed (SQL fragment).
const NOT_EXPIRED: &str = "expires_at > strftime('%Y-%m-%dT%H:%M:%fZ', 'now')";

pub async fn create_invite(
    State(state): State<AppState>,
    Path(family_id): Path<Uuid>,
    headers: HeaderMap,
    Json(input): Json<CreateInvite>,
) -> ApiResult<InviteExpiry> {
    let caller = Device::authenticate_owner(&state.db, &headers, family_id).await?;
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
        expires_at_utc: column(&row, "expires_at")?,
    }))
}

/// The new phone asks to join, proving it knows the invitation's secret.
pub async fn request_join(
    State(state): State<AppState>,
    Path(invite_id): Path<Uuid>,
    Json(input): Json<InviteProof>,
) -> ApiResult<InviteStatus> {
    let verifier = decode(&input.verifier, Some(32))?;
    let key_package = decode(&input.key_package, None)?;
    if key_package.is_empty() || key_package.len() > MAX_KEY_PACKAGE_BYTES {
        return Err(bad_request("invalid key package size"));
    }
    let result = sqlx::query(&format!(
        "UPDATE invitations SET status='pending', pending_device_id=?, key_package=? \
         WHERE id=? AND verifier_hash=? AND status='open' AND {NOT_EXPIRED}"
    ))
    .bind(input.device_id.to_string())
    .bind(&key_package)
    .bind(invite_id.to_string())
    .bind(verifier)
    .execute(&state.db)
    .await
    .map_err(internal)?;
    if result.rows_affected() != 1 {
        return Err(gone("invitation unavailable"));
    }
    Ok(Json(InviteStatus {
        status: InviteState::Pending,
        pending_device_id: Some(input.device_id),
        pending_key_package: Some(input.key_package),
    }))
}

pub async fn invite_status(
    State(state): State<AppState>,
    Path((family_id, invite_id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
) -> ApiResult<InviteStatus> {
    Device::authenticate_owner(&state.db, &headers, family_id).await?;
    let row = sqlx::query(&format!(
        "SELECT status, pending_device_id, key_package FROM invitations \
         WHERE id=? AND family_id=? AND {NOT_EXPIRED}"
    ))
    .bind(invite_id.to_string())
    .bind(family_id.to_string())
    .fetch_optional(&state.db)
    .await
    .map_err(internal)?
    .ok_or(gone("invitation unavailable"))?;
    let status: String = column(&row, "status")?;
    let status = serde_json::from_value(serde_json::Value::String(status)).map_err(|_| {
        ApiError::new(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "invalid invitation state",
        )
    })?;
    let pending_device_id = match column::<Option<String>>(&row, "pending_device_id")? {
        Some(_) => Some(uuid_column(&row, "pending_device_id")?),
        None => None,
    };
    let pending_key_package = column::<Option<Vec<u8>>>(&row, "key_package")?
        .as_deref()
        .map(encode);
    Ok(Json(InviteStatus {
        status,
        pending_device_id,
        pending_key_package,
    }))
}

/// The head, after comparing the six digits, leaves the welcome here.
pub async fn approve_join(
    State(state): State<AppState>,
    Path((family_id, invite_id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    Json(input): Json<ApproveJoin>,
) -> ApiResult<InviteStatus> {
    Device::authenticate_owner(&state.db, &headers, family_id).await?;
    let welcome = decode(&input.welcome, None)?;
    if welcome.is_empty() || welcome.len() > MAX_WELCOME_BYTES {
        return Err(bad_request("invalid welcome size"));
    }
    // The new member reads from after the commit that added it; its history
    // comes from the head as a snapshot.
    let result = sqlx::query(&format!(
        "UPDATE invitations SET status='approved', welcome=?, \
         joined_after=(SELECT COALESCE(MAX(sequence), 0) FROM group_messages WHERE family_id=?) \
         WHERE id=? AND family_id=? AND pending_device_id=? AND status='pending' AND {NOT_EXPIRED}"
    ))
    .bind(&welcome)
    .bind(family_id.to_string())
    .bind(invite_id.to_string())
    .bind(family_id.to_string())
    .bind(input.device_id.to_string())
    .execute(&state.db)
    .await
    .map_err(internal)?;
    if result.rows_affected() != 1 {
        return Err(gone("invitation unavailable"));
    }
    Ok(Json(InviteStatus {
        status: InviteState::Approved,
        pending_device_id: Some(input.device_id),
        pending_key_package: None,
    }))
}

pub async fn claim_invite(
    State(state): State<AppState>,
    Path(invite_id): Path<Uuid>,
    Json(input): Json<ClaimInvite>,
) -> ApiResult<ClaimedInvite> {
    let verifier = decode(&input.verifier, Some(32))?;
    let token_hash = decode(&input.token_hash, Some(32))?;
    let mut tx = state.db.begin().await.map_err(internal)?;
    let row = sqlx::query(&format!(
        "SELECT family_id, status, welcome, joined_after \
         FROM invitations WHERE id=? AND verifier_hash=? AND pending_device_id=? \
         AND status IN ('approved', 'used') AND {NOT_EXPIRED}"
    ))
    .bind(invite_id.to_string())
    .bind(verifier)
    .bind(input.device_id.to_string())
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal)?
    .ok_or(gone("invitation unavailable"))?;
    let family_id = uuid_column(&row, "family_id")?;
    if column::<String>(&row, "status")? == "approved" {
        sqlx::query("INSERT INTO devices (id, family_id, token_hash) VALUES (?,?,?)")
            .bind(input.device_id.to_string())
            .bind(family_id.to_string())
            .bind(&token_hash)
            .execute(&mut *tx)
            .await
            .map_err(internal)?;
        sqlx::query("UPDATE invitations SET status='used' WHERE id=?")
            .bind(invite_id.to_string())
            .execute(&mut *tx)
            .await
            .map_err(internal)?;
    } else {
        // Already used: only the same phone with the same token may ask again.
        sqlx::query("SELECT 1 FROM devices WHERE id=? AND family_id=? AND token_hash=?")
            .bind(input.device_id.to_string())
            .bind(family_id.to_string())
            .bind(&token_hash)
            .fetch_optional(&mut *tx)
            .await
            .map_err(internal)?
            .ok_or(gone("invitation unavailable"))?;
    }
    tx.commit().await.map_err(internal)?;
    Ok(Json(ClaimedInvite {
        family_id,
        welcome: encode(&column::<Vec<u8>>(&row, "welcome")?),
        after: column(&row, "joined_after")?,
    }))
}

pub async fn cancel_invite(
    State(state): State<AppState>,
    Path((family_id, invite_id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
) -> ApiResult<InviteStatus> {
    Device::authenticate_owner(&state.db, &headers, family_id).await?;
    sqlx::query(
        "UPDATE invitations SET status='cancelled' \
         WHERE id=? AND family_id=? AND status IN ('open','pending','approved')",
    )
    .bind(invite_id.to_string())
    .bind(family_id.to_string())
    .execute(&state.db)
    .await
    .map_err(internal)?;
    Ok(Json(InviteStatus {
        status: InviteState::Cancelled,
        pending_device_id: None,
        pending_key_package: None,
    }))
}
