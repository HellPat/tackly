//! Who is calling: a device presenting its bearer token. Taking a device out,
//! and moving to a new token.

use serde::{Deserialize, Serialize};
use tackly_wire::{RotateToken, token_hash};
use uuid::Uuid;

use crate::{
    db::args,
    error::{ApiError, ApiResult, forbidden, unauthorized},
    relay::{Call, json, stored_uuid},
};

pub struct Device {
    pub id: Uuid,
    pub owner: bool,
    /// An AI agent: reads, never writes.
    pub agent: bool,
}

#[derive(Deserialize)]
struct DeviceRow {
    id: String,
    is_owner: i64,
    is_agent: i64,
}

impl Device {
    /// The calling device, if its token belongs to `family_id`, it was not
    /// taken out, and (an agent) its time is not over.
    pub async fn authenticate(call: &Call<'_>, family_id: Uuid) -> Result<Self, ApiError> {
        let token = call.bearer().ok_or(unauthorized("device token required"))?;
        let row: DeviceRow = call
            .db
            .first(
                "SELECT id, is_owner, is_agent FROM devices \
                 WHERE family_id = ? AND token_hash = ? AND revoked_at IS NULL \
                 AND (expires_at IS NULL OR expires_at > strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
                args![family_id, token_hash(&token).as_bytes().as_slice()],
            )
            .await?
            .ok_or(unauthorized("invalid device token"))?;
        Ok(Self {
            id: stored_uuid(&row.id)?,
            owner: row.is_owner != 0,
            agent: row.is_agent != 0,
        })
    }

    /// Like [`Self::authenticate`], and the device must be the family's head.
    pub async fn authenticate_owner(call: &Call<'_>, family_id: Uuid) -> Result<Self, ApiError> {
        let device = Self::authenticate(call, family_id).await?;
        if device.owner {
            Ok(device)
        } else {
            Err(forbidden("owner required"))
        }
    }
}

#[derive(Serialize)]
struct Revoked {
    device_id: Uuid,
}

/// The head takes a device out of the family: its token stops working. The
/// head's own device cannot be taken out.
pub async fn revoke(call: &mut Call<'_>, family_id: Uuid, device_id: Uuid) -> ApiResult {
    let caller = Device::authenticate_owner(call, family_id).await?;
    if caller.id == device_id {
        return Err(forbidden("the head stays"));
    }
    call.db
        .run(
            "UPDATE devices SET revoked_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') \
             WHERE id = ? AND family_id = ? AND is_owner = 0 AND revoked_at IS NULL",
            args![device_id, family_id],
        )
        .await?;
    json(&Revoked { device_id })
}

/// The calling device moves to a new token: the old one (on a lost phone,
/// say) stops working at once.
pub async fn rotate(call: &mut Call<'_>, family_id: Uuid) -> ApiResult {
    let caller = Device::authenticate(call, family_id).await?;
    let input: RotateToken = call.body().await?;
    call.db
        .run(
            "UPDATE devices SET token_hash = ? WHERE id = ? AND family_id = ?",
            args![input.token_hash.as_bytes().as_slice(), caller.id, family_id],
        )
        .await?;
    json(&Revoked {
        device_id: caller.id,
    })
}
