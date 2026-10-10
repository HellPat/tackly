//! Domain events. The app appends them, encrypts them under the family key,
//! and replays them into [`crate::FamilyState`]. The server relays them as
//! opaque [`crate::wire::EncryptedEvent`]s and never sees these types in
//! plaintext.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// One fact recorded by a device. The CQRS aggregate is the family; the
/// `subject_id` names the family, list, or task the fact is about. The whole
/// event is the encrypted body; the routing IDs are repeated outside the
/// ciphertext (as the wire `subject_id`) and authenticated as associated
/// data.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FamilyEvent {
    pub id: Uuid,
    pub subject_id: Uuid,
    pub origin_device_id: Uuid,
    pub occurred_at: DateTime<Utc>,
    #[serde(flatten)]
    pub event: DomainEvent,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum DomainEvent {
    /// Aggregate: the family. Origin device is the owner.
    #[serde(rename = "family.created")]
    FamilyCreated { name: String, owner_name: String },
    /// Aggregate: the family. Origin device is the member who joined.
    #[serde(rename = "member.joined")]
    MemberJoined { name: String },
    #[serde(rename = "list.created")]
    ListCreated { name: String, emoji: String },
    #[serde(rename = "task.created")]
    TaskCreated {
        list_id: Uuid,
        title: String,
        emoji: String,
    },
    /// Someone began working on the task. Shown live to other members.
    #[serde(rename = "task.started")]
    TaskStarted,
    #[serde(rename = "task.completed")]
    TaskCompleted(CompletionMetadata),
    #[serde(rename = "task.reopened")]
    TaskReopened { completion_event_id: Uuid },
    /// Settles competing completions of one task. Only a member who wrote
    /// one of the competing completions may resolve; replay ignores others.
    #[serde(rename = "task.conflict_resolved")]
    TaskConflictResolved {
        kept_completion_event_id: Uuid,
        resolved_event_ids: Vec<Uuid>,
    },
    /// Written by newer app versions; older replays skip it.
    #[serde(other)]
    Unknown,
}

/// Recorded on the finishing device at the moment of completion.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CompletionMetadata {
    /// The `task.started` event this completion closes, if the finisher
    /// started it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_event_id: Option<Uuid>,
    /// Seconds between that start and the completion.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_seconds: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<GeoPoint>,
}

impl CompletionMetadata {
    /// True when `other` records everything this one does: every detail set
    /// here is unset or identical there.
    pub fn is_covered_by(&self, other: &Self) -> bool {
        fn covered<T: PartialEq>(mine: &Option<T>, theirs: &Option<T>) -> bool {
            mine.is_none() || mine == theirs
        }
        covered(&self.started_event_id, &other.started_event_id)
            && covered(&self.duration_seconds, &other.duration_seconds)
            && covered(&self.note, &other.note)
            && covered(&self.location, &other.location)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct GeoPoint {
    pub latitude: f64,
    pub longitude: f64,
    pub accuracy_meters: Option<f64>,
}

impl cqrs_es::DomainEvent for FamilyEvent {
    fn event_type(&self) -> String {
        self.event.type_name().to_owned()
    }

    fn event_version(&self) -> String {
        "1".into()
    }
}

impl DomainEvent {
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::FamilyCreated { .. } => "family.created",
            Self::MemberJoined { .. } => "member.joined",
            Self::ListCreated { .. } => "list.created",
            Self::TaskCreated { .. } => "task.created",
            Self::TaskStarted => "task.started",
            Self::TaskCompleted(_) => "task.completed",
            Self::TaskReopened { .. } => "task.reopened",
            Self::TaskConflictResolved { .. } => "task.conflict_resolved",
            Self::Unknown => "unknown",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completion_round_trips_with_metadata() {
        let envelope = FamilyEvent {
            id: Uuid::new_v4(),
            subject_id: Uuid::new_v4(),
            origin_device_id: Uuid::new_v4(),
            occurred_at: Utc::now(),
            event: DomainEvent::TaskCompleted(CompletionMetadata {
                started_event_id: Some(Uuid::new_v4()),
                duration_seconds: Some(90),
                note: Some("Used the blue bin".into()),
                location: Some(GeoPoint {
                    latitude: 52.52,
                    longitude: 13.405,
                    accuracy_meters: Some(8.0),
                }),
            }),
        };
        let json = serde_json::to_value(&envelope).unwrap();
        assert_eq!(json["type"], "task.completed");
        assert_eq!(json["duration_seconds"], 90);
        let back: FamilyEvent = serde_json::from_value(json).unwrap();
        assert_eq!(back, envelope);
    }

    #[test]
    fn unknown_event_types_do_not_break_replay() {
        let json = serde_json::json!({
            "id": Uuid::new_v4(),
            "subject_id": Uuid::new_v4(),
            "origin_device_id": Uuid::new_v4(),
            "occurred_at": Utc::now(),
            "type": "task.photographed",
            "photo": "…",
        });
        let event: FamilyEvent = serde_json::from_value(json).unwrap();
        assert_eq!(event.event, DomainEvent::Unknown);
    }
}
