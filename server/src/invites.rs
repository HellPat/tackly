//! Letting a new phone join a family.
//!
//! An invitation goes open → pending (a phone asked) → approved (the owner
//! sealed the family key for it) → used. The relay only moves the sealed key
//! package along; the secret that opens it never reaches the relay.

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

const MAX_KEY_PACKAGE_BYTES: usize = 4096;

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
    let result = sqlx::query(&format!(
        "UPDATE invitations SET status='pending', pending_device_id=? \
         WHERE id=? AND verifier_hash=? AND status='open' AND {NOT_EXPIRED}"
    ))
    .bind(input.device_id.to_string())
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
    }))
}

pub async fn invite_status(
    State(state): State<AppState>,
    Path((family_id, invite_id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
) -> ApiResult<InviteStatus> {
    Device::authenticate_owner(&state.db, &headers, family_id).await?;
    let row = sqlx::query(&format!(
        "SELECT status, pending_device_id FROM invitations \
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
    Ok(Json(InviteStatus {
        status,
        pending_device_id,
    }))
}

/// The owner, after comparing the six digits, hands over the sealed key.
pub async fn approve_join(
    State(state): State<AppState>,
    Path((family_id, invite_id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    Json(input): Json<ApproveJoin>,
) -> ApiResult<InviteStatus> {
    Device::authenticate_owner(&state.db, &headers, family_id).await?;
    let nonce = decode(&input.package_nonce, Some(12))?;
    let ciphertext = decode(&input.package_ciphertext, None)?;
    let mac = decode(&input.package_mac, Some(16))?;
    if ciphertext.is_empty() || ciphertext.len() > MAX_KEY_PACKAGE_BYTES {
        return Err(bad_request("invalid key package size"));
    }
    let result = sqlx::query(&format!(
        "UPDATE invitations SET status='approved', package_nonce=?, package_ciphertext=?, package_mac=? \
         WHERE id=? AND family_id=? AND pending_device_id=? AND status='pending' AND {NOT_EXPIRED}"
    ))
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
        return Err(gone("invitation unavailable"));
    }
    Ok(Json(InviteStatus {
        status: InviteState::Approved,
        pending_device_id: Some(input.device_id),
    }))
}

/// The new phone collects the sealed key and becomes a device of the family.
/// It chose its own token and sends the hash, so asking again after a lost
/// answer returns the same package instead of failing.
pub async fn claim_invite(
    State(state): State<AppState>,
    Path(invite_id): Path<Uuid>,
    Json(input): Json<ClaimInvite>,
) -> ApiResult<ClaimedInvite> {
    let verifier = decode(&input.verifier, Some(32))?;
    let token_hash = decode(&input.token_hash, Some(32))?;
    let mut tx = state.db.begin().await.map_err(internal)?;
    let row = sqlx::query(&format!(
        "SELECT family_id, status, package_nonce, package_ciphertext, package_mac \
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
        package_nonce: encode(&column::<Vec<u8>>(&row, "package_nonce")?),
        package_ciphertext: encode(&column::<Vec<u8>>(&row, "package_ciphertext")?),
        package_mac: encode(&column::<Vec<u8>>(&row, "package_mac")?),
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
    }))
}
