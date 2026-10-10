//! HTTP and SSE payloads. Binary values are unpadded base64url strings.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// SSE event name for a batch of [`SequencedEvent`]s in an [`EventsPage`].
pub const SSE_EVENTS: &str = "events";
/// Maximum events per POST.
pub const MAX_APPEND_BATCH: usize = 8;
/// Maximum events per GET page and per SSE message.
pub const MAX_PAGE: i64 = 20;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CreateFamily {
    pub family_id: Uuid,
    pub device_id: Uuid,
    pub recovery_verifier: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeviceToken {
    pub device_token: String,
    pub owner: bool,
}

/// An event as the server stores it. The server can read only the IDs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EncryptedEvent {
    pub event_id: Uuid,
    pub aggregate_id: Uuid,
    pub origin_device_id: Uuid,
    pub key_version: i32,
    pub nonce: String,
    pub ciphertext: String,
    pub mac: String,
}

impl EncryptedEvent {
    /// Associated data binding the ciphertext to its routing IDs.
    pub fn associated_data(&self, family_id: Uuid) -> String {
        format!(
            "{family_id}:{}:{}:{}",
            self.event_id, self.aggregate_id, self.origin_device_id
        )
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppendEvents {
    pub events: Vec<EncryptedEvent>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppendResult {
    pub accepted: usize,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct EventsQuery {
    pub after: Option<i64>,
    pub limit: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SequencedEvent {
    pub sequence: i64,
    #[serde(flatten)]
    pub event: EncryptedEvent,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct EventsPage {
    pub events: Vec<SequencedEvent>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CreateInvite {
    pub invite_id: Uuid,
    pub verifier: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InviteExpiry {
    pub expires_at_utc: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InviteProof {
    pub verifier: String,
    pub device_id: Uuid,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InviteState {
    Open,
    Pending,
    Approved,
    Used,
    Cancelled,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InviteStatus {
    pub status: InviteState,
    pub pending_device_id: Option<Uuid>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ApproveJoin {
    pub device_id: Uuid,
    pub package_nonce: String,
    pub package_ciphertext: String,
    pub package_mac: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ClaimInvite {
    pub verifier: String,
    pub device_id: Uuid,
    pub recovery_verifier: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ClaimedInvite {
    pub family_id: Uuid,
    pub device_token: String,
    pub package_nonce: String,
    pub package_ciphertext: String,
    pub package_mac: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ApiErrorBody {
    pub error: String,
}
