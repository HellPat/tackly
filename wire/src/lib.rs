//! The messages between a Tackly phone and the relay: HTTP and WebSocket
//! payloads.
//! Binary values are unpadded base64url strings. Nothing here knows the
//! family: the relay sees only these.

mod binary;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub use binary::{BinaryError, Bytes, Digest};

/// Maximum events per POST.
pub const MAX_APPEND_BATCH: usize = 8;
/// Maximum events per GET page and per live message.
pub const MAX_PAGE: i64 = 20;

/// What the relay keeps of a device token: the token itself stays on the phone,
/// which chooses it. The SHA-256 of the token's text.
pub fn token_hash(token: &str) -> Digest {
    Digest::sha256(token.as_bytes())
}

/// Registers a family and its first device. The phone chose the token and
/// sends only its hash, so repeating the request (the answer may have been
/// lost) is harmless.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CreateFamily {
    pub family_id: Uuid,
    pub device_id: Uuid,
    pub token_hash: Digest,
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
    /// The MLS message.
    pub message: Bytes,
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
    pub verifier: Digest,
}

/// The longest an AI agent may be let in for.
pub const MAX_AGENT_SECONDS: i64 = 30 * 24 * 60 * 60;
/// How many requests for an AI agent's access wait for a family at once.
pub const MAX_PENDING_AGENT_REQUESTS: i64 = 5;

/// An AI agent asks a family for access. It knows the relay and the family's
/// ID, nothing secret; the family's head decides after comparing the code
/// both show (derived from `key_package`).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentAccessRequest {
    pub request_id: Uuid,
    pub device_id: Uuid,
    /// The agent's MLS key package (base64url).
    pub key_package: Bytes,
}

/// A request waiting for the head, as the head's phone reads it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PendingAgentRequest {
    pub request_id: Uuid,
    pub device_id: Uuid,
    pub key_package: Bytes,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PendingAgentRequests {
    pub requests: Vec<PendingAgentRequest>,
}

/// The head lets the agent in for `agent_seconds`, and leaves its MLS welcome.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ApproveAgent {
    pub agent_seconds: i64,
    pub welcome: Bytes,
}

/// The agent asks how its request is doing; with the token hash it chose, it
/// gets its access once the head allowed it.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ClaimAgent {
    pub device_id: Uuid,
    pub token_hash: Digest,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentRequestState {
    Pending,
    Approved,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentClaim {
    pub status: AgentRequestState,
    #[serde(default)]
    pub family_id: Option<Uuid>,
    #[serde(default)]
    pub welcome: Option<Bytes>,
    /// The relay sequence the agent reads after (the history is a snapshot).
    #[serde(default)]
    pub after: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InviteExpiry {
    pub expires_at_utc: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InviteProof {
    pub verifier: Digest,
    pub device_id: Uuid,
    /// The joining phone's MLS key package (base64url). Both phones derive
    /// their six digits from it, so a server that swaps it is caught.
    pub key_package: Bytes,
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
    pub pending_key_package: Option<Bytes>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ApproveJoin {
    pub device_id: Uuid,
    /// The MLS welcome that lets the phone into the group (base64url).
    pub welcome: Bytes,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ClaimInvite {
    pub verifier: Digest,
    pub device_id: Uuid,
    pub token_hash: Digest,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ClaimedInvite {
    pub family_id: Uuid,
    pub welcome: Bytes,
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
    pub blob: Bytes,
}

/// The largest recovery backup the relay keeps.
pub const MAX_RECOVERY_BYTES: usize = 16 * 1024 * 1024;

/// A device moves to a new token (after coming back from a backup, so that
/// the lost phone's token stops working).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RotateToken {
    pub token_hash: Digest,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ApiErrorBody {
    pub error: String,
}
