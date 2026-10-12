//! An AI agent asks a family for access.
//!
//! The agent needs nothing secret: the relay's address and the family's ID.
//! It leaves a request (its MLS key package); the family's head sees it on its
//! phone, compares the code both show (a hash of that key package, so a relay
//! that swaps the key is caught), and allows it for a time or denies it. The
//! relay only keeps the request, the head's welcome and the agent's token. It
//! is the same shape as the OAuth device flow (RFC 8628), with the family's
//! MLS group doing the encryption.

use axum::{
    Json,
    extract::{Path, State},
    http::HeaderMap,
};
use tackly_wire::{
    AgentAccessRequest, AgentClaim, AgentRequestState, ApproveAgent, Bytes, ClaimAgent,
    MAX_AGENT_SECONDS, MAX_PENDING_AGENT_REQUESTS, PendingAgentRequest, PendingAgentRequests,
};
use uuid::Uuid;

use crate::{
    AppState,
    auth::Device,
    db::{column, uuid_column},
    error::{ApiResult, bad_request, forbidden, gone, internal, not_found, too_many},
};

const MAX_KEY_PACKAGE_BYTES: usize = 8 * 1024;
/// A welcome carries the group's tree: it grows with the family.
const MAX_WELCOME_BYTES: usize = 256 * 1024;

/// True while the request's ten minutes have not passed (SQL fragment).
const NOT_EXPIRED: &str = "expires_at > strftime('%Y-%m-%dT%H:%M:%fZ', 'now')";

/// The answer to a call that only needs to say it worked.
#[derive(serde::Serialize)]
pub struct Asked {
    request_id: Uuid,
}

/// The agent asks. No token: it has none yet. At most a few requests wait for
/// a family at once, so that nobody can bury the head's phone in them.
pub async fn ask(
    State(state): State<AppState>,
    Path(family_id): Path<Uuid>,
    Json(input): Json<AgentAccessRequest>,
) -> ApiResult<Asked> {
    let key_package = input.key_package.as_slice().to_vec();
    if key_package.is_empty() || key_package.len() > MAX_KEY_PACKAGE_BYTES {
        return Err(bad_request("invalid key package size"));
    }
    let mut tx = state.db.begin().await.map_err(internal)?;
    // Requests that ran out a day ago are of no use to anyone.
    sqlx::query(
        "DELETE FROM agent_requests \
         WHERE expires_at < strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-1 day')",
    )
    .execute(&mut *tx)
    .await
    .map_err(internal)?;
    sqlx::query("SELECT 1 FROM families WHERE id = ?")
        .bind(family_id.to_string())
        .fetch_optional(&mut *tx)
        .await
        .map_err(internal)?
        .ok_or(not_found("no such family"))?;
    let waiting: i64 = column(
        &sqlx::query(&format!(
            "SELECT COUNT(*) AS n FROM agent_requests \
             WHERE family_id = ? AND status = 'pending' AND {NOT_EXPIRED}"
        ))
        .bind(family_id.to_string())
        .fetch_one(&mut *tx)
        .await
        .map_err(internal)?,
        "n",
    )?;
    if waiting >= MAX_PENDING_AGENT_REQUESTS {
        return Err(too_many("too many requests wait for this family"));
    }
    sqlx::query(
        "INSERT INTO agent_requests (id, family_id, device_id, key_package) VALUES (?,?,?,?)",
    )
    .bind(input.request_id.to_string())
    .bind(family_id.to_string())
    .bind(input.device_id.to_string())
    .bind(&key_package)
    .execute(&mut *tx)
    .await
    .map_err(internal)?;
    tx.commit().await.map_err(internal)?;
    Ok(Json(Asked {
        request_id: input.request_id,
    }))
}

/// What waits for the head.
pub async fn pending(
    State(state): State<AppState>,
    Path(family_id): Path<Uuid>,
    headers: HeaderMap,
) -> ApiResult<PendingAgentRequests> {
    Device::authenticate_owner(&state.db, &headers, family_id).await?;
    let rows = sqlx::query(&format!(
        "SELECT id, device_id, key_package FROM agent_requests \
         WHERE family_id = ? AND status = 'pending' AND {NOT_EXPIRED} ORDER BY created_at"
    ))
    .bind(family_id.to_string())
    .fetch_all(&state.db)
    .await
    .map_err(internal)?;
    let requests = rows
        .iter()
        .map(|row| {
            Ok(PendingAgentRequest {
                request_id: uuid_column(row, "id")?,
                device_id: uuid_column(row, "device_id")?,
                key_package: Bytes::from(column::<Vec<u8>>(row, "key_package")?),
            })
        })
        .collect::<Result<_, crate::error::ApiError>>()?;
    Ok(Json(PendingAgentRequests { requests }))
}

/// The head allows the agent for a time and leaves the welcome.
pub async fn approve(
    State(state): State<AppState>,
    Path((family_id, request_id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    Json(input): Json<ApproveAgent>,
) -> ApiResult<Asked> {
    Device::authenticate_owner(&state.db, &headers, family_id).await?;
    if !(1..=MAX_AGENT_SECONDS).contains(&input.agent_seconds) {
        return Err(bad_request("an agent reads for a second to thirty days"));
    }
    let welcome = input.welcome.as_slice().to_vec();
    if welcome.is_empty() || welcome.len() > MAX_WELCOME_BYTES {
        return Err(bad_request("invalid welcome size"));
    }
    // The agent reads from after the commit that added it; its history comes
    // from the head as a snapshot.
    let result = sqlx::query(&format!(
        "UPDATE agent_requests SET status = 'approved', welcome = ?1, \
         agent_expires_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '+' || ?2 || ' seconds'), \
         joined_after = (SELECT COALESCE(MAX(sequence), 0) FROM group_messages WHERE family_id = ?3) \
         WHERE id = ?4 AND family_id = ?3 AND status = 'pending' AND {NOT_EXPIRED}"
    ))
    .bind(&welcome)
    .bind(input.agent_seconds)
    .bind(family_id.to_string())
    .bind(request_id.to_string())
    .execute(&state.db)
    .await
    .map_err(internal)?;
    if result.rows_affected() != 1 {
        return Err(gone("request unavailable"));
    }
    Ok(Json(Asked { request_id }))
}

pub async fn deny(
    State(state): State<AppState>,
    Path((family_id, request_id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
) -> ApiResult<Asked> {
    Device::authenticate_owner(&state.db, &headers, family_id).await?;
    sqlx::query(
        "UPDATE agent_requests SET status = 'denied' \
         WHERE id = ? AND family_id = ? AND status = 'pending'",
    )
    .bind(request_id.to_string())
    .bind(family_id.to_string())
    .execute(&state.db)
    .await
    .map_err(internal)?;
    Ok(Json(Asked { request_id }))
}

/// The agent asks how it is doing. Once allowed, it gets its access: the relay
/// records the token hash the agent chose, which ends when its time is over.
pub async fn claim(
    State(state): State<AppState>,
    Path(request_id): Path<Uuid>,
    Json(input): Json<ClaimAgent>,
) -> ApiResult<AgentClaim> {
    let token_hash = input.token_hash.as_bytes().to_vec();
    let mut tx = state.db.begin().await.map_err(internal)?;
    let row = sqlx::query(
        "SELECT family_id, status, welcome, joined_after, agent_expires_at, expires_at, \
         expires_at > strftime('%Y-%m-%dT%H:%M:%fZ', 'now') AS alive \
         FROM agent_requests WHERE id = ? AND device_id = ?",
    )
    .bind(request_id.to_string())
    .bind(input.device_id.to_string())
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal)?
    .ok_or(gone("request unavailable"))?;
    let status: String = column(&row, "status")?;
    let alive = column::<i64>(&row, "alive")? != 0;
    match status.as_str() {
        "pending" if alive => {
            return Ok(Json(AgentClaim {
                status: AgentRequestState::Pending,
                family_id: None,
                welcome: None,
                after: None,
            }));
        }
        "approved" | "used" => {}
        _ => return Err(gone("request unavailable")),
    }
    let family_id = uuid_column(&row, "family_id")?;
    if status == "approved" {
        sqlx::query(
            "INSERT INTO devices (id, family_id, token_hash, is_agent, expires_at) \
             VALUES (?,?,?,1,?)",
        )
        .bind(input.device_id.to_string())
        .bind(family_id.to_string())
        .bind(&token_hash)
        .bind(column::<Option<String>>(&row, "agent_expires_at")?)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
        sqlx::query("UPDATE agent_requests SET status = 'used' WHERE id = ?")
            .bind(request_id.to_string())
            .execute(&mut *tx)
            .await
            .map_err(internal)?;
    } else {
        // Already used: only the same agent with the same token may ask again.
        sqlx::query("SELECT 1 FROM devices WHERE id=? AND family_id=? AND token_hash=?")
            .bind(input.device_id.to_string())
            .bind(family_id.to_string())
            .bind(&token_hash)
            .fetch_optional(&mut *tx)
            .await
            .map_err(internal)?
            .ok_or(forbidden("request unavailable"))?;
    }
    tx.commit().await.map_err(internal)?;
    Ok(Json(AgentClaim {
        status: AgentRequestState::Approved,
        family_id: Some(family_id),
        welcome: Some(Bytes::from(column::<Vec<u8>>(&row, "welcome")?)),
        after: Some(column(&row, "joined_after")?),
    }))
}
