//! Registering a family.

use axum::{Json, extract::State};
use serde::Serialize;
use tackly_protocol::wire::CreateFamily;

use crate::{
    AppState,
    db::decode,
    error::{ApiResult, conflict, internal},
};

#[derive(Serialize)]
pub struct Registered {
    owner: bool,
}

/// Creates a family with its first device as the owner. The token's hash comes
/// from the phone, so the same request repeated (because the answer was lost)
/// is recognised and answered the same way; any other request for an existing
/// family is a conflict.
pub async fn create_family(
    State(state): State<AppState>,
    Json(input): Json<CreateFamily>,
) -> ApiResult<Registered> {
    let token_hash = decode(&input.token_hash, Some(32))?;
    let mut tx = state.db.begin().await.map_err(internal)?;
    let inserted = sqlx::query("INSERT INTO families (id) VALUES (?) ON CONFLICT DO NOTHING")
        .bind(input.family_id.to_string())
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
    if inserted.rows_affected() == 0 {
        let same_registration = sqlx::query(
            "SELECT 1 FROM devices WHERE id = ? AND family_id = ? AND token_hash = ? \
             AND is_owner = 1 AND revoked_at IS NULL",
        )
        .bind(input.device_id.to_string())
        .bind(input.family_id.to_string())
        .bind(&token_hash)
        .fetch_optional(&mut *tx)
        .await
        .map_err(internal)?;
        return match same_registration {
            Some(_) => Ok(Json(Registered { owner: true })),
            None => Err(conflict("family exists")),
        };
    }
    sqlx::query("INSERT INTO devices (id, family_id, token_hash, is_owner) VALUES (?, ?, ?, 1)")
        .bind(input.device_id.to_string())
        .bind(input.family_id.to_string())
        .bind(&token_hash)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
    tx.commit().await.map_err(internal)?;
    Ok(Json(Registered { owner: true }))
}
