//! The head's recovery backup: an opaque blob under an ID the relay cannot
//! connect to a recovery code. The head leaves and removes it; a phone that
//! knows the code fetches it.

use serde::{Deserialize, Serialize};
use tackly_wire::{Bytes, Digest, MAX_RECOVERY_BYTES, RecoveryBlob};
use uuid::Uuid;

use crate::{
    auth::Device,
    db::args,
    error::{ApiResult, bad_request, not_found},
    relay::{Call, json},
};

#[derive(Serialize)]
struct Stored {
    id: Digest,
}

#[derive(Deserialize)]
struct BlobRow {
    blob: Vec<u8>,
}

pub async fn put(call: &mut Call<'_>, family_id: Uuid, id: Digest) -> ApiResult {
    Device::authenticate_owner(call, family_id).await?;
    let input: RecoveryBlob = call.body()?;
    let blob = input.blob.as_slice();
    if blob.is_empty() || blob.len() > MAX_RECOVERY_BYTES {
        return Err(bad_request("invalid backup size"));
    }
    let changed = call
        .db
        .run(
            "INSERT INTO recovery_blobs (id, family_id, blob) VALUES (?1, ?2, ?3) \
             ON CONFLICT (id) DO UPDATE SET blob = excluded.blob, \
             updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') \
             WHERE recovery_blobs.family_id = ?2",
            args![id.to_string(), family_id, blob],
        )
        .await?;
    if changed == 0 {
        return Err(bad_request("that ID belongs to another family"));
    }
    json(&Stored { id })
}

pub async fn delete(call: &mut Call<'_>, family_id: Uuid, id: Digest) -> ApiResult {
    Device::authenticate_owner(call, family_id).await?;
    call.db
        .run(
            "DELETE FROM recovery_blobs WHERE id = ? AND family_id = ?",
            args![id.to_string(), family_id],
        )
        .await?;
    json(&Stored { id })
}

/// No token: whoever knows the recovery code knows the ID, and only they can
/// open what they get.
pub async fn get(call: &mut Call<'_>, id: Digest) -> ApiResult {
    let row: BlobRow = call
        .db
        .first(
            "SELECT blob FROM recovery_blobs WHERE id = ?",
            args![id.to_string()],
        )
        .await?
        .ok_or(not_found("no backup under this ID"))?;
    json(&RecoveryBlob {
        blob: Bytes::from(row.blob),
    })
}
