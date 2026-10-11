//! The head's recovery backup: an opaque blob under an ID the relay cannot
//! connect to a recovery code. The head leaves and removes it; a phone that
//! knows the code fetches it.

use axum::{
    Json,
    extract::{Path, State},
    http::HeaderMap,
};
use tackly_wire::{MAX_RECOVERY_BYTES, RecoveryBlob};
use uuid::Uuid;

use crate::{
    AppState,
    auth::Device,
    db::{column, decode, encode},
    error::{ApiError, ApiResult, bad_request, internal},
};

/// A base64url ID of 32 bytes, as the phones derive it.
fn valid_id(id: &str) -> Result<(), ApiError> {
    decode(id, Some(32)).map(|_| ())
}

#[derive(serde::Serialize)]
pub struct Stored {
    id: String,
}

pub async fn put_recovery(
    State(state): State<AppState>,
    Path((family_id, id)): Path<(Uuid, String)>,
    headers: HeaderMap,
    Json(input): Json<RecoveryBlob>,
) -> ApiResult<Stored> {
    Device::authenticate_owner(&state.db, &headers, family_id).await?;
    valid_id(&id)?;
    let blob = decode(&input.blob, None)?;
    if blob.is_empty() || blob.len() > MAX_RECOVERY_BYTES {
        return Err(bad_request("invalid backup size"));
    }
    let result = sqlx::query(
        "INSERT INTO recovery_blobs (id, family_id, blob) VALUES (?1, ?2, ?3) \
         ON CONFLICT (id) DO UPDATE SET blob = excluded.blob, \
         updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') \
         WHERE recovery_blobs.family_id = ?2",
    )
    .bind(&id)
    .bind(family_id.to_string())
    .bind(&blob)
    .execute(&state.db)
    .await
    .map_err(internal)?;
    if result.rows_affected() == 0 {
        return Err(bad_request("that ID belongs to another family"));
    }
    Ok(Json(Stored { id }))
}

pub async fn delete_recovery(
    State(state): State<AppState>,
    Path((family_id, id)): Path<(Uuid, String)>,
    headers: HeaderMap,
) -> ApiResult<Stored> {
    Device::authenticate_owner(&state.db, &headers, family_id).await?;
    valid_id(&id)?;
    sqlx::query("DELETE FROM recovery_blobs WHERE id = ? AND family_id = ?")
        .bind(&id)
        .bind(family_id.to_string())
        .execute(&state.db)
        .await
        .map_err(internal)?;
    Ok(Json(Stored { id }))
}

/// No token: whoever knows the recovery code knows the ID, and only they
/// can open what they get.
pub async fn get_recovery(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<RecoveryBlob> {
    valid_id(&id)?;
    let row = sqlx::query("SELECT blob FROM recovery_blobs WHERE id = ?")
        .bind(&id)
        .fetch_optional(&state.db)
        .await
        .map_err(internal)?
        .ok_or(ApiError::new(
            axum::http::StatusCode::NOT_FOUND,
            "no backup under this ID",
        ))?;
    Ok(Json(RecoveryBlob {
        blob: encode(&column::<Vec<u8>>(&row, "blob")?),
    }))
}
