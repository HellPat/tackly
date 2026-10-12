//! The head's recovery backup: an opaque blob under an ID the relay cannot
//! connect to a recovery code. The head leaves and removes it; a phone that
//! knows the code fetches it.

use axum::{
    Json,
    extract::{Path, State},
    http::HeaderMap,
};
use tackly_wire::{Bytes, Digest, MAX_RECOVERY_BYTES, RecoveryBlob};
use uuid::Uuid;

use crate::{
    AppState,
    auth::Device,
    db::column,
    error::{ApiError, ApiResult, bad_request, internal},
};

#[derive(serde::Serialize)]
pub struct Stored {
    id: Digest,
}

pub async fn put_recovery(
    State(state): State<AppState>,
    Path((family_id, id)): Path<(Uuid, Digest)>,
    headers: HeaderMap,
    Json(input): Json<RecoveryBlob>,
) -> ApiResult<Stored> {
    Device::authenticate_owner(&state.db, &headers, family_id).await?;
    let blob = input.blob.as_slice().to_vec();
    if blob.is_empty() || blob.len() > MAX_RECOVERY_BYTES {
        return Err(bad_request("invalid backup size"));
    }
    let result = sqlx::query(
        "INSERT INTO recovery_blobs (id, family_id, blob) VALUES (?1, ?2, ?3) \
         ON CONFLICT (id) DO UPDATE SET blob = excluded.blob, \
         updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') \
         WHERE recovery_blobs.family_id = ?2",
    )
    .bind(id.to_string())
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
    Path((family_id, id)): Path<(Uuid, Digest)>,
    headers: HeaderMap,
) -> ApiResult<Stored> {
    Device::authenticate_owner(&state.db, &headers, family_id).await?;
    sqlx::query("DELETE FROM recovery_blobs WHERE id = ? AND family_id = ?")
        .bind(id.to_string())
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
    Path(id): Path<Digest>,
) -> ApiResult<RecoveryBlob> {
    let row = sqlx::query("SELECT blob FROM recovery_blobs WHERE id = ?")
        .bind(id.to_string())
        .fetch_optional(&state.db)
        .await
        .map_err(internal)?
        .ok_or(ApiError::new(
            axum::http::StatusCode::NOT_FOUND,
            "no backup under this ID",
        ))?;
    Ok(Json(RecoveryBlob {
        blob: Bytes::from(column::<Vec<u8>>(&row, "blob")?),
    }))
}
