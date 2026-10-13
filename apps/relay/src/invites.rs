//! Letting a new phone join a family.
//!
//! An invitation goes open → pending (a phone asked, with its MLS key
//! package) → approved (the head added it to the group and left the welcome
//! here) → used. The relay only moves key package and welcome along; the
//! secret both phones derive their digits from never reaches it.

use serde::Deserialize;
use tackly_wire::{
    ApproveJoin, Bytes, ClaimInvite, ClaimedInvite, CreateInvite, InviteExpiry, InviteProof,
    InviteState, InviteStatus,
};
use uuid::Uuid;

use crate::{
    auth::Device,
    db::args,
    error::{ApiResult, bad_request, gone, internal},
    relay::{Call, json, stored_uuid},
};

const MAX_KEY_PACKAGE_BYTES: usize = 8 * 1024;
/// A welcome carries the group's tree: it grows with the family.
const MAX_WELCOME_BYTES: usize = 256 * 1024;

/// True while the invitation's five minutes have not passed (SQL fragment).
const NOT_EXPIRED: &str = "expires_at > strftime('%Y-%m-%dT%H:%M:%fZ', 'now')";

#[derive(Deserialize)]
struct ExpiryRow {
    expires_at: String,
}

pub async fn create(call: &mut Call<'_>, family_id: Uuid) -> ApiResult {
    let caller = Device::authenticate_owner(call, family_id).await?;
    let input: CreateInvite = call.body()?;
    let row: ExpiryRow = call
        .db
        .first(
            "INSERT INTO invitations (id, family_id, verifier_hash, created_by) \
             VALUES (?, ?, ?, ?) RETURNING expires_at",
            args![
                input.invite_id,
                family_id,
                input.verifier.as_bytes().as_slice(),
                caller.id,
            ],
        )
        .await?
        .ok_or_else(|| internal("the invitation was not stored"))?;
    json(&InviteExpiry {
        expires_at_utc: row.expires_at,
    })
}

/// The new phone asks to join, proving it knows the invitation's secret.
pub async fn request(call: &mut Call<'_>, invite_id: Uuid) -> ApiResult {
    let input: InviteProof = call.body()?;
    let key_package = input.key_package.as_slice();
    if key_package.is_empty() || key_package.len() > MAX_KEY_PACKAGE_BYTES {
        return Err(bad_request("invalid key package size"));
    }
    let changed = call
        .db
        .run(
            &format!(
                "UPDATE invitations SET status = 'pending', pending_device_id = ?, key_package = ? \
                 WHERE id = ? AND verifier_hash = ? AND status = 'open' AND {NOT_EXPIRED}"
            ),
            args![
                input.device_id,
                key_package,
                invite_id,
                input.verifier.as_bytes().as_slice(),
            ],
        )
        .await?;
    if changed != 1 {
        return Err(gone("invitation unavailable"));
    }
    json(&InviteStatus {
        status: InviteState::Pending,
        pending_device_id: Some(input.device_id),
        pending_key_package: Some(input.key_package),
    })
}

#[derive(Deserialize)]
struct StatusRow {
    status: String,
    pending_device_id: Option<String>,
    key_package: Option<Vec<u8>>,
}

pub async fn status(call: &mut Call<'_>, family_id: Uuid, invite_id: Uuid) -> ApiResult {
    Device::authenticate_owner(call, family_id).await?;
    let row: StatusRow = call
        .db
        .first(
            &format!(
                "SELECT status, pending_device_id, key_package FROM invitations \
                 WHERE id = ? AND family_id = ? AND {NOT_EXPIRED}"
            ),
            args![invite_id, family_id],
        )
        .await?
        .ok_or(gone("invitation unavailable"))?;
    let status = serde_json::from_value(serde_json::Value::String(row.status))
        .map_err(|_| internal("invalid invitation state"))?;
    json(&InviteStatus {
        status,
        pending_device_id: row
            .pending_device_id
            .as_deref()
            .map(stored_uuid)
            .transpose()?,
        pending_key_package: row.key_package.map(Bytes::from),
    })
}

/// The head, after comparing the six digits, leaves the welcome here.
pub async fn approve(call: &mut Call<'_>, family_id: Uuid, invite_id: Uuid) -> ApiResult {
    Device::authenticate_owner(call, family_id).await?;
    let input: ApproveJoin = call.body()?;
    let welcome = input.welcome.as_slice();
    if welcome.is_empty() || welcome.len() > MAX_WELCOME_BYTES {
        return Err(bad_request("invalid welcome size"));
    }
    // The new member reads from after the commit that added it; its history
    // comes from the head as a snapshot.
    let changed = call
        .db
        .run(
            &format!(
                "UPDATE invitations SET status = 'approved', welcome = ?1, \
                 joined_after = (SELECT COALESCE(MAX(sequence), 0) FROM group_messages \
                                 WHERE family_id = ?2) \
                 WHERE id = ?3 AND family_id = ?2 AND pending_device_id = ?4 \
                 AND status = 'pending' AND {NOT_EXPIRED}"
            ),
            args![welcome, family_id, invite_id, input.device_id],
        )
        .await?;
    if changed != 1 {
        return Err(gone("invitation unavailable"));
    }
    json(&InviteStatus {
        status: InviteState::Approved,
        pending_device_id: Some(input.device_id),
        pending_key_package: None,
    })
}

#[derive(Deserialize)]
struct ClaimRow {
    family_id: String,
    status: String,
    welcome: Vec<u8>,
    joined_after: i64,
}

#[derive(Deserialize)]
struct Found {}

pub async fn claim(call: &mut Call<'_>, invite_id: Uuid) -> ApiResult {
    let input: ClaimInvite = call.body()?;
    let token_hash = input.token_hash.as_bytes().as_slice();
    let row: ClaimRow = call
        .db
        .first(
            &format!(
                "SELECT family_id, status, welcome, joined_after FROM invitations \
                 WHERE id = ? AND verifier_hash = ? AND pending_device_id = ? \
                 AND status IN ('approved', 'used') AND {NOT_EXPIRED}"
            ),
            args![
                invite_id,
                input.verifier.as_bytes().as_slice(),
                input.device_id,
            ],
        )
        .await?
        .ok_or(gone("invitation unavailable"))?;
    let family_id = stored_uuid(&row.family_id)?;
    if row.status == "approved" {
        call.db
            .batch(vec![
                call.db.statement(
                    "INSERT INTO devices (id, family_id, token_hash) VALUES (?, ?, ?)",
                    args![input.device_id, family_id, token_hash],
                )?,
                call.db.statement(
                    "UPDATE invitations SET status = 'used' WHERE id = ?",
                    args![invite_id],
                )?,
            ])
            .await?;
    } else {
        // Already used: only the same phone with the same token may ask again.
        call.db
            .first::<Found>(
                "SELECT 1 AS found FROM devices WHERE id = ? AND family_id = ? AND token_hash = ?",
                args![input.device_id, family_id, token_hash],
            )
            .await?
            .ok_or(gone("invitation unavailable"))?;
    }
    json(&ClaimedInvite {
        family_id,
        welcome: Bytes::from(row.welcome),
        after: row.joined_after,
    })
}

pub async fn cancel(call: &mut Call<'_>, family_id: Uuid, invite_id: Uuid) -> ApiResult {
    Device::authenticate_owner(call, family_id).await?;
    call.db
        .run(
            "UPDATE invitations SET status = 'cancelled' \
             WHERE id = ? AND family_id = ? AND status IN ('open', 'pending', 'approved')",
            args![invite_id, family_id],
        )
        .await?;
    json(&InviteStatus {
        status: InviteState::Cancelled,
        pending_device_id: None,
        pending_key_package: None,
    })
}
