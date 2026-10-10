//! Who is calling: a device presenting its bearer token.

use axum::http::{HeaderMap, header::AUTHORIZATION};
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::{
    db::{column, uuid_column},
    error::{ApiError, forbidden, internal, unauthorized},
};

pub struct Device {
    pub id: Uuid,
    pub owner: bool,
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
            "SELECT id, is_owner FROM devices \
             WHERE family_id = ? AND token_hash = ? AND revoked_at IS NULL",
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
