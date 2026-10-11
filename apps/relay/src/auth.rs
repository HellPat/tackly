//! Who is calling: a device presenting its bearer token.

use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, header::AUTHORIZATION},
};
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use tackly_wire::RotateToken;
use uuid::Uuid;

use crate::{
    AppState,
    db::{column, decode, uuid_column},
    error::{ApiError, ApiResult, forbidden, internal, unauthorized},
};

pub struct Device {
    pub id: Uuid,
    pub owner: bool,
    /// An AI agent: reads, never writes.
    pub agent: bool,
}

impl Device {
    /// The calling device, if its token belongs to `family_id`.
    pub async fn authenticate(
        db: &SqlitePool,
        headers: &HeaderMap,
        family_id: Uuid,
    ) -> Result<Self, ApiError> {
        let token = headers
            .get(AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "))
            .ok_or(unauthorized("device token required"))?;
        let row = sqlx::query(
            "SELECT id, is_owner, is_agent FROM devices \
             WHERE family_id = ? AND token_hash = ? AND revoked_at IS NULL \
             AND (expires_at IS NULL OR expires_at > strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
        )
        .bind(family_id.to_string())
        .bind(Sha256::digest(token.as_bytes()).to_vec())
        .fetch_optional(db)
        .await
        .map_err(internal)?
        .ok_or(unauthorized("invalid device token"))?;
        Ok(Self {
            id: uuid_column(&row, "id")?,
            owner: column::<i64>(&row, "is_owner")? != 0,
            agent: column::<i64>(&row, "is_agent")? != 0,
        })
    }

    /// Like [`Self::authenticate`], and the device must be the family's owner.
    pub async fn authenticate_owner(
        db: &SqlitePool,
        headers: &HeaderMap,
        family_id: Uuid,
    ) -> Result<Self, ApiError> {
        let device = Self::authenticate(db, headers, family_id).await?;
        if device.owner {
            Ok(device)
        } else {
            Err(forbidden("owner required"))
        }
    }
}

/// The head takes a device out of the family: its token stops working. The
/// head's own device cannot be taken out.
pub async fn revoke_device(
    State(state): State<AppState>,
    Path((family_id, device_id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
) -> ApiResult<Revoked> {
    let caller = Device::authenticate_owner(&state.db, &headers, family_id).await?;
    if caller.id == device_id {
        return Err(forbidden("the head stays"));
    }
    sqlx::query(
        "UPDATE devices SET revoked_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') \
         WHERE id = ? AND family_id = ? AND is_owner = 0 AND revoked_at IS NULL",
    )
    .bind(device_id.to_string())
    .bind(family_id.to_string())
    .execute(&state.db)
    .await
    .map_err(internal)?;
    Ok(Json(Revoked { device_id }))
}

#[derive(serde::Serialize)]
pub struct Revoked {
    device_id: Uuid,
}

/// The calling device moves to a new token: the old one (on a lost phone,
/// say) stops working at once.
pub async fn rotate_token(
    State(state): State<AppState>,
    Path(family_id): Path<Uuid>,
    headers: HeaderMap,
    Json(input): Json<RotateToken>,
) -> ApiResult<Revoked> {
    let caller = Device::authenticate(&state.db, &headers, family_id).await?;
    let token_hash = decode(&input.token_hash, Some(32))?;
    sqlx::query("UPDATE devices SET token_hash = ? WHERE id = ? AND family_id = ?")
        .bind(token_hash)
        .bind(caller.id.to_string())
        .bind(family_id.to_string())
        .execute(&state.db)
        .await
        .map_err(internal)?;
    Ok(Json(Revoked {
        device_id: caller.id,
    }))
}
