//! The messages between a Tackly phone and the relay: HTTP and SSE payloads.
//! Binary values are unpadded base64url strings. Nothing here knows the
//! family: the relay sees only these.

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

/// SSE event name for a batch of [`SequencedEvent`]s in an [`EventsPage`].
pub const SSE_EVENTS: &str = "events";
/// Maximum events per POST.
pub const MAX_APPEND_BATCH: usize = 8;
/// Maximum events per GET page and per SSE message.
pub const MAX_PAGE: i64 = 20;

/// What the relay keeps of a device token: the token itself stays on the phone,
/// which chooses it. Base64url of the SHA-256 of the token's text.
pub fn token_hash(token: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(token.as_bytes()))
}

/// Registers a family and its first device. The phone chose the token and
/// sends only its hash, so repeating the request (the answer may have been
/// lost) is harmless.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CreateFamily {
    pub family_id: Uuid,
    pub device_id: Uuid,
    pub token_hash: String,
}

/// A message to the family's MLS group, as the server stores it: an
/// application message (an event, or a snapshot of the history for a new
/// member) or a commit (someone added or removed). The server reads only the
/// two IDs, the group ID, the epoch and the kind of message. The IDs are
/// random (UUIDv4): a time-ordered ID would tell it when the action really
/// happened, even if the phone uploaded it days later.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EncryptedEvent {
    pub event_id: Uuid,
    pub origin_device_id: Uuid,
    /// The MLS message (base64url).
    pub message: String,
}

/// The server's answer to a message encrypted for an epoch that is over: the
/// phone catches up with the group and encrypts again.
pub const STALE_EPOCH: &str = "stale epoch";
/// The largest MLS message the server takes (a snapshot chunk stays below it).
pub const MAX_MESSAGE_BYTES: usize = 2 * 1024 * 1024;

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
    /// Set for an AI agent: it may read for this long, then its token ends.
    #[serde(default)]
    pub agent_seconds: Option<i64>,
}

/// The longest an AI agent may be let in for.
pub const MAX_AGENT_SECONDS: i64 = 30 * 24 * 60 * 60;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InviteExpiry {
    pub expires_at_utc: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InviteProof {
    pub verifier: String,
    pub device_id: Uuid,
    /// The joining phone's MLS key package (base64url). Both phones derive
    /// their six digits from it, so a server that swaps it is caught.
    pub key_package: String,
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
    #[serde(default)]
    pub pending_key_package: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ApproveJoin {
    pub device_id: Uuid,
    /// The MLS welcome that lets the phone into the group (base64url).
    pub welcome: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ClaimInvite {
    pub verifier: String,
    pub device_id: Uuid,
    pub token_hash: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ClaimedInvite {
    pub family_id: Uuid,
    pub welcome: String,
    /// The relay sequence the new member starts reading after (its history
    /// comes as a snapshot from the head).
    pub after: i64,
}

/// The head's recovery backup: sealed on the phone with a key from the
/// recovery code, kept by the relay under an ID derived from the same code.
/// The relay cannot open it.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RecoveryBlob {
    /// base64url
    pub blob: String,
}

/// The largest recovery backup the relay keeps.
pub const MAX_RECOVERY_BYTES: usize = 16 * 1024 * 1024;

/// A device moves to a new token (after coming back from a backup, so that
/// the lost phone's token stops working).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RotateToken {
    pub token_hash: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ApiErrorBody {
    pub error: String,
}
