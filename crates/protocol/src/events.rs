//! Domain events. The app appends them, sends them to the family's MLS group,
//! and replays them into [`crate::Family`]. The server relays them as opaque
//! [`crate::wire::EncryptedEvent`]s and never sees these types in plaintext.

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// One fact recorded by a device. The CQRS aggregate is the family; the
/// `subject_id` names what the fact is about: the family, a list, a place
/// group, a place, a task, a problem, or (for a day's plan) the member. The
/// whole event is the encrypted body.
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
    // ---- the family and its members (subject: the family; origin: the member)
    #[serde(rename = "family.created")]
    FamilyCreated { name: String, owner_name: String },
    #[serde(rename = "member.joined")]
    MemberJoined { name: String },
    #[serde(rename = "member.renamed")]
    MemberRenamed { name: String },
    #[serde(rename = "member.picture_changed")]
    MemberPictureChanged { picture: Picture },
    /// The head took a member (or an AI agent) out of the family. Only the
    /// head's word counts: replay ignores it from anyone else.
    #[serde(rename = "member.removed")]
    MemberRemoved { member_id: Uuid },
    /// The head let an AI agent in, to read along until `until`. Only the
    /// head's word counts.
    #[serde(rename = "agent.admitted")]
    AgentAdmitted {
        member_id: Uuid,
        name: String,
        until: DateTime<Utc>,
    },

    // ---- task lists. Tasks without a list are in "Other", which always exists.
    #[serde(rename = "list.created")]
    ListCreated { name: String },
    #[serde(rename = "list.renamed")]
    ListRenamed { name: String },
    /// Only a list without open tasks can be deleted.
    #[serde(rename = "list.deleted")]
    ListDeleted,

    // ---- places, in groups such as "Grocery Store"
    #[serde(rename = "place_group.created")]
    PlaceGroupCreated { name: String },
    #[serde(rename = "place_group.renamed")]
    PlaceGroupRenamed { name: String },
    /// Only a group without places can be deleted.
    #[serde(rename = "place_group.deleted")]
    PlaceGroupDeleted,
    #[serde(rename = "place.created")]
    PlaceCreated {
        group_id: Uuid,
        name: String,
        /// Every place starts with one location.
        location: PlaceLocation,
    },
    #[serde(rename = "place.renamed")]
    PlaceRenamed { name: String },
    #[serde(rename = "place.moved")]
    PlaceMoved { group_id: Uuid },
    #[serde(rename = "place.deleted")]
    PlaceDeleted,
    #[serde(rename = "place.location_added")]
    PlaceLocationAdded { location: PlaceLocation },
    /// A place always keeps at least one location.
    #[serde(rename = "place.location_removed")]
    PlaceLocationRemoved { location_id: Uuid },

    // ---- tasks
    #[serde(rename = "task.created")]
    TaskCreated {
        /// `None`: the task is in "Other".
        #[serde(default, skip_serializing_if = "Option::is_none")]
        list_id: Option<Uuid>,
        title: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        place_ids: Vec<Uuid>,
        /// A step that fixes this problem (made when the problem was noticed).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        problem_id: Option<Uuid>,
    },
    #[serde(rename = "task.deleted")]
    TaskDeleted,
    #[serde(rename = "task.place_added")]
    TaskPlaceAdded { place_id: Uuid },
    #[serde(rename = "task.place_removed")]
    TaskPlaceRemoved { place_id: Uuid },
    /// Someone takes the task, or is given it. `None` gives it back.
    #[serde(rename = "task.assigned")]
    TaskAssigned { member_id: Option<Uuid> },
    /// The origin works on it from now on (and so has it).
    #[serde(rename = "task.started")]
    TaskStarted,
    #[serde(rename = "task.paused")]
    TaskPaused,
    /// Where the phone was when it was finished, if it could tell. Kept for
    /// later use; the app does not show it.
    #[serde(rename = "task.completed")]
    TaskCompleted {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        location: Option<GeoPoint>,
    },
    /// Where the phone was when it was finished, once it knew: a GPS fix
    /// takes seconds, and finishing never waits for it.
    #[serde(rename = "task.completion_located")]
    TaskCompletionLocated { location: GeoPoint },
    /// Undo right after finishing: everything as it was, time included.
    #[serde(rename = "task.completion_undone")]
    TaskCompletionUndone,
    /// Open again later: the time worked starts over.
    #[serde(rename = "task.reopened")]
    TaskReopened,

    // ---- problems: things that go wrong at home, each with its solutions
    #[serde(rename = "problem.created")]
    ProblemCreated { title: String, icon: String },
    #[serde(rename = "problem.renamed")]
    ProblemRenamed { title: String },
    #[serde(rename = "problem.solution_added")]
    ProblemSolutionAdded { solution: Solution },
    #[serde(rename = "problem.solution_removed")]
    ProblemSolutionRemoved { solution_id: Uuid },
    /// It is back: noticed by the origin, for `member_id` (the origin itself, or
    /// someone it is raised for). Each solution becomes a step, a task of its own.
    #[serde(rename = "problem.noticed")]
    ProblemNoticed { member_id: Uuid },
    #[serde(rename = "problem.assigned")]
    ProblemAssigned { member_id: Uuid },
    #[serde(rename = "problem.resolved")]
    ProblemResolved,
    /// Undo of resolving: open again, with the same steps.
    #[serde(rename = "problem.reopened")]
    ProblemReopened,

    // ---- plans (subject: the member who plans)
    /// The origin's focus for a day, in order. A later plan for the same day replaces it.
    #[serde(rename = "day.planned")]
    DayPlanned { day: NaiveDate, task_ids: Vec<Uuid> },

    /// Written by newer app versions; older replays skip it.
    #[serde(other)]
    Unknown,
}

/// A person's picture: an icon (a Material Symbols name) on one of a few
/// tints, or a photo. The photo wins when there is one.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Picture {
    pub icon: String,
    pub tint: u8,
    /// A small square JPEG as a data URL (`data:image/jpeg;base64,…`),
    /// made on the phone from the camera or the gallery.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub photo: Option<String>,
}

/// The largest photo a picture may carry, in bytes of its data URL. A 256 px
/// square JPEG is far smaller; this keeps a family's history small.
pub const MAX_PHOTO_BYTES: usize = 150_000;
const PHOTO_PREFIX: &str = "data:image/jpeg;base64,";

impl Picture {
    /// The photo is a JPEG data URL of a sensible size (or there is none).
    pub fn photo_is_valid(&self) -> bool {
        self.photo.as_ref().is_none_or(|photo| {
            photo.len() <= MAX_PHOTO_BYTES
                && photo.strip_prefix(PHOTO_PREFIX).is_some_and(|data| {
                    !data.is_empty()
                        && data
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b"+/=".contains(&b))
                })
        })
    }
}

/// One physical spot of a place, such as the LIDL in Winnenden. Looked up by
/// the app when online; without coordinates it is just a name.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlaceLocation {
    pub id: Uuid,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub point: Option<GeoPoint>,
}

impl PlaceLocation {
    /// A location known only by its name (no address, no coordinates).
    pub fn named(name: impl Into<String>) -> Self {
        Self {
            id: Uuid::now_v7(),
            name: name.into(),
            address: None,
            point: None,
        }
    }
}

/// One way to fix a problem. When the problem is noticed it becomes a task.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Solution {
    pub id: Uuid,
    pub title: String,
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
            Self::MemberRenamed { .. } => "member.renamed",
            Self::MemberPictureChanged { .. } => "member.picture_changed",
            Self::MemberRemoved { .. } => "member.removed",
            Self::AgentAdmitted { .. } => "agent.admitted",
            Self::ListCreated { .. } => "list.created",
            Self::ListRenamed { .. } => "list.renamed",
            Self::ListDeleted => "list.deleted",
            Self::PlaceGroupCreated { .. } => "place_group.created",
            Self::PlaceGroupRenamed { .. } => "place_group.renamed",
            Self::PlaceGroupDeleted => "place_group.deleted",
            Self::PlaceCreated { .. } => "place.created",
            Self::PlaceRenamed { .. } => "place.renamed",
            Self::PlaceMoved { .. } => "place.moved",
            Self::PlaceDeleted => "place.deleted",
            Self::PlaceLocationAdded { .. } => "place.location_added",
            Self::PlaceLocationRemoved { .. } => "place.location_removed",
            Self::TaskCreated { .. } => "task.created",
            Self::TaskDeleted => "task.deleted",
            Self::TaskPlaceAdded { .. } => "task.place_added",
            Self::TaskPlaceRemoved { .. } => "task.place_removed",
            Self::TaskAssigned { .. } => "task.assigned",
            Self::TaskStarted => "task.started",
            Self::TaskPaused => "task.paused",
            Self::TaskCompleted { .. } => "task.completed",
            Self::TaskCompletionLocated { .. } => "task.completion_located",
            Self::TaskCompletionUndone => "task.completion_undone",
            Self::TaskReopened => "task.reopened",
            Self::ProblemCreated { .. } => "problem.created",
            Self::ProblemRenamed { .. } => "problem.renamed",
            Self::ProblemSolutionAdded { .. } => "problem.solution_added",
            Self::ProblemSolutionRemoved { .. } => "problem.solution_removed",
            Self::ProblemNoticed { .. } => "problem.noticed",
            Self::ProblemAssigned { .. } => "problem.assigned",
            Self::ProblemResolved => "problem.resolved",
            Self::ProblemReopened => "problem.reopened",
            Self::DayPlanned { .. } => "day.planned",
            Self::Unknown => "unknown",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_task_in_other_round_trips_without_a_list() {
        let envelope = FamilyEvent {
            id: Uuid::now_v7(),
            subject_id: Uuid::now_v7(),
            origin_device_id: Uuid::now_v7(),
            occurred_at: Utc::now(),
            event: DomainEvent::TaskCreated {
                list_id: None,
                title: "Milk".into(),
                place_ids: vec![],
                problem_id: None,
            },
        };
        let json = serde_json::to_value(&envelope).unwrap();
        assert_eq!(json["type"], "task.created");
        assert!(json.get("list_id").is_none());
        let back: FamilyEvent = serde_json::from_value(json).unwrap();
        assert_eq!(back, envelope);
    }

    #[test]
    fn unknown_event_types_do_not_break_replay() {
        let json = serde_json::json!({
            "id": Uuid::now_v7(),
            "subject_id": Uuid::now_v7(),
            "origin_device_id": Uuid::now_v7(),
            "occurred_at": Utc::now(),
            "type": "task.photographed",
            "photo": "…",
        });
        let event: FamilyEvent = serde_json::from_value(json).unwrap();
        assert_eq!(event.event, DomainEvent::Unknown);
    }
}
