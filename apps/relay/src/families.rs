//! Registering a family.

use serde::{Deserialize, Serialize};
use tackly_wire::CreateFamily;

use crate::{
    db::args,
    error::{ApiResult, conflict},
    relay::{Call, json},
};

#[derive(Serialize)]
struct Registered {
    owner: bool,
}

#[derive(Deserialize)]
struct Found {}

/// Creates a family with its first device as the head. The token's hash comes
/// from the phone, so the same request repeated (because the answer was lost)
/// is recognised and answered the same way; any other request for an existing
/// family is a conflict.
pub async fn create(call: &mut Call<'_>) -> ApiResult {
    let input: CreateFamily = call.body()?;
    let token_hash = input.token_hash.as_bytes().as_slice();
    let exists = call
        .db
        .first::<Found>(
            "SELECT 1 AS found FROM families WHERE id = ?",
            args![input.family_id],
        )
        .await?
        .is_some();
    if exists {
        let same_registration = call
            .db
            .first::<Found>(
                "SELECT 1 AS found FROM devices WHERE id = ? AND family_id = ? AND token_hash = ? \
                 AND is_owner = 1 AND revoked_at IS NULL",
                args![input.device_id, input.family_id, token_hash],
            )
            .await?
            .is_some();
        return if same_registration {
            json(&Registered { owner: true })
        } else {
            Err(conflict("family exists"))
        };
    }
    call.db
        .batch(vec![
            call.db.statement(
                "INSERT INTO families (id) VALUES (?)",
                args![input.family_id],
            )?,
            call.db.statement(
                "INSERT INTO devices (id, family_id, token_hash, is_owner) VALUES (?, ?, ?, 1)",
                args![input.device_id, input.family_id, token_hash],
            )?,
        ])
        .await?;
    json(&Registered { owner: true })
}
