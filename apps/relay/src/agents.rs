//! An AI agent asks a family for access.
//!
//! The agent needs nothing secret: the relay's address and the family's ID.
//! It leaves a request (its MLS key package); the family's head sees it on its
//! phone, compares the code both show (a hash of that key package, so a relay
//! that swaps the key is caught), and allows it for a time or denies it. The
//! relay only keeps the request, the head's welcome and the agent's token. It
//! is the same shape as the OAuth device flow (RFC 8628), with the family's
//! MLS group doing the encryption.

use serde::{Deserialize, Serialize};
use tackly_wire::{
    AgentAccessRequest, AgentClaim, AgentRequestState, ApproveAgent, Bytes, ClaimAgent,
    MAX_AGENT_SECONDS, MAX_PENDING_AGENT_REQUESTS, PendingAgentRequest, PendingAgentRequests,
};
use uuid::Uuid;

use crate::{
    auth::Device,
    db::args,
    error::{ApiError, ApiResult, bad_request, forbidden, gone, not_found, too_many},
    relay::{Call, json, stored_uuid},
};

const MAX_KEY_PACKAGE_BYTES: usize = 8 * 1024;
/// A welcome carries the group's tree: it grows with the family.
const MAX_WELCOME_BYTES: usize = 256 * 1024;

/// True while the request's ten minutes have not passed (SQL fragment).
const NOT_EXPIRED: &str = "expires_at > strftime('%Y-%m-%dT%H:%M:%fZ', 'now')";

/// The answer to a call that only needs to say it worked.
#[derive(Serialize)]
struct Asked {
    request_id: Uuid,
}

#[derive(Deserialize)]
struct Found {}

#[derive(Deserialize)]
struct Count {
    n: i64,
}

/// The agent asks. No token: it has none yet. At most a few requests wait for
/// a family at once, so that nobody can bury the head's phone in them.
pub async fn ask(call: &mut Call<'_>, family_id: Uuid) -> ApiResult {
    let input: AgentAccessRequest = call.body().await?;
    let key_package = input.key_package.as_slice();
    if key_package.is_empty() || key_package.len() > MAX_KEY_PACKAGE_BYTES {
        return Err(bad_request("invalid key package size"));
    }
    let db = call.db;
    db.first::<Found>(
        "SELECT 1 AS found FROM families WHERE id = ?",
        args![family_id],
    )
    .await?
    .ok_or(not_found("no such family"))?;
    let waiting = db
        .first::<Count>(
            &format!(
                "SELECT COUNT(*) AS n FROM agent_requests \
                 WHERE family_id = ? AND status = 'pending' AND {NOT_EXPIRED}"
            ),
            args![family_id],
        )
        .await?
        .map_or(0, |count| count.n);
    if waiting >= MAX_PENDING_AGENT_REQUESTS {
        return Err(too_many("too many requests wait for this family"));
    }
    db.batch(vec![
        // Requests that ran out a day ago are of no use to anyone.
        db.statement(
            "DELETE FROM agent_requests \
             WHERE expires_at < strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-1 day')",
            args![],
        )?,
        db.statement(
            "INSERT INTO agent_requests (id, family_id, device_id, key_package) \
             VALUES (?, ?, ?, ?)",
            args![input.request_id, family_id, input.device_id, key_package],
        )?,
    ])
    .await?;
    json(&Asked {
        request_id: input.request_id,
    })
}

#[derive(Deserialize)]
struct PendingRow {
    id: String,
    device_id: String,
    key_package: Vec<u8>,
}

/// What waits for the head.
pub async fn pending(call: &mut Call<'_>, family_id: Uuid) -> ApiResult {
    Device::authenticate_owner(call, family_id).await?;
    let rows: Vec<PendingRow> = call
        .db
        .all(
            &format!(
                "SELECT id, device_id, key_package FROM agent_requests \
                 WHERE family_id = ? AND status = 'pending' AND {NOT_EXPIRED} ORDER BY created_at"
            ),
            args![family_id],
        )
        .await?;
    let requests = rows
        .into_iter()
        .map(|row| {
            Ok(PendingAgentRequest {
                request_id: stored_uuid(&row.id)?,
                device_id: stored_uuid(&row.device_id)?,
                key_package: Bytes::from(row.key_package),
            })
        })
        .collect::<Result<_, ApiError>>()?;
    json(&PendingAgentRequests { requests })
}

/// The head allows the agent for a time and leaves the welcome.
pub async fn approve(call: &mut Call<'_>, family_id: Uuid, request_id: Uuid) -> ApiResult {
    Device::authenticate_owner(call, family_id).await?;
    let input: ApproveAgent = call.body().await?;
    if !(1..=MAX_AGENT_SECONDS).contains(&input.agent_seconds) {
        return Err(bad_request("an agent reads for a second to thirty days"));
    }
    let welcome = input.welcome.as_slice();
    if welcome.is_empty() || welcome.len() > MAX_WELCOME_BYTES {
        return Err(bad_request("invalid welcome size"));
    }
    // The agent reads from after the commit that added it; its history comes
    // from the head as a snapshot.
    let changed = call
        .db
        .run(
            &format!(
                "UPDATE agent_requests SET status = 'approved', welcome = ?1, \
                 agent_expires_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '+' || ?2 || ' seconds'), \
                 joined_after = (SELECT COALESCE(MAX(sequence), 0) FROM group_messages \
                                 WHERE family_id = ?3) \
                 WHERE id = ?4 AND family_id = ?3 AND status = 'pending' AND {NOT_EXPIRED}"
            ),
            args![welcome, input.agent_seconds, family_id, request_id],
        )
        .await?;
    if changed != 1 {
        return Err(gone("request unavailable"));
    }
    json(&Asked { request_id })
}

pub async fn deny(call: &mut Call<'_>, family_id: Uuid, request_id: Uuid) -> ApiResult {
    Device::authenticate_owner(call, family_id).await?;
    call.db
        .run(
            "UPDATE agent_requests SET status = 'denied' \
             WHERE id = ? AND family_id = ? AND status = 'pending'",
            args![request_id, family_id],
        )
        .await?;
    json(&Asked { request_id })
}

#[derive(Deserialize)]
struct ClaimRow {
    family_id: String,
    status: String,
    welcome: Option<Vec<u8>>,
    joined_after: Option<i64>,
    agent_expires_at: Option<String>,
    alive: i64,
}

/// The agent asks how it is doing. Once allowed, it gets its access: the relay
/// records the token hash the agent chose, which ends when its time is over.
pub async fn claim(call: &mut Call<'_>, request_id: Uuid) -> ApiResult {
    let input: ClaimAgent = call.body().await?;
    let token_hash = input.token_hash.as_bytes().as_slice();
    let db = call.db;
    let row: ClaimRow = db
        .first(
            "SELECT family_id, status, welcome, joined_after, agent_expires_at, \
             expires_at > strftime('%Y-%m-%dT%H:%M:%fZ', 'now') AS alive \
             FROM agent_requests WHERE id = ? AND device_id = ?",
            args![request_id, input.device_id],
        )
        .await?
        .ok_or(gone("request unavailable"))?;
    match row.status.as_str() {
        "pending" if row.alive != 0 => {
            return json(&AgentClaim {
                status: AgentRequestState::Pending,
                family_id: None,
                welcome: None,
                after: None,
            });
        }
        "approved" | "used" => {}
        _ => return Err(gone("request unavailable")),
    }
    let family_id = stored_uuid(&row.family_id)?;
    if row.status == "approved" {
        db.batch(vec![
            db.statement(
                "INSERT INTO devices (id, family_id, token_hash, is_agent, expires_at) \
                 VALUES (?, ?, ?, 1, ?)",
                args![input.device_id, family_id, token_hash, row.agent_expires_at],
            )?,
            db.statement(
                "UPDATE agent_requests SET status = 'used' WHERE id = ?",
                args![request_id],
            )?,
        ])
        .await?;
    } else {
        // Already used: only the same agent with the same token may ask again.
        db.first::<Found>(
            "SELECT 1 AS found FROM devices WHERE id = ? AND family_id = ? AND token_hash = ?",
            args![input.device_id, family_id, token_hash],
        )
        .await?
        .ok_or(forbidden("request unavailable"))?;
    }
    json(&AgentClaim {
        status: AgentRequestState::Approved,
        family_id: Some(family_id),
        welcome: row.welcome.map(Bytes::from),
        after: row.joined_after,
    })
}
