//! The family as it stands: replayed from events. Events are applied in server
//! order, followed by this device's not-yet-uploaded events. Replay never
//! fails: an event that no longer fits (a second completion, a start on a done
//! task, a delete that would orphan something) is ignored, so every device
//! that sees the same events in the same order ends up with the same state.

use std::collections::BTreeMap;

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{
    events::FamilyEvent,
    members::Member,
    places::{Place, PlaceGroup},
    problems::Problem,
    tasks::{Task, TaskList},
};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Family {
    pub family_id: Option<Uuid>,
    pub name: Option<String>,
    pub members: BTreeMap<Uuid, Member>,
    /// IDs are UUIDv7, so these maps are in creation order.
    pub lists: BTreeMap<Uuid, TaskList>,
    pub place_groups: BTreeMap<Uuid, PlaceGroup>,
    pub places: BTreeMap<Uuid, Place>,
    pub tasks: BTreeMap<Uuid, Task>,
    pub problems: BTreeMap<Uuid, Problem>,
    /// Each member's focus per day, in their order (tasks as planned, done or not).
    pub plans: BTreeMap<Uuid, BTreeMap<NaiveDate, Vec<Uuid>>>,
}

impl Family {
    /// Rebuilds the family by replaying events in order.
    pub fn replay<'a>(events: impl IntoIterator<Item = &'a FamilyEvent>) -> Self {
        let mut state = Self::default();
        for event in events {
            state.apply_event(event);
        }
        state
    }

    /// Applies one event. Events that do not fit the current state are
    /// ignored (see the module documentation). Each area of the family knows
    /// its own events.
    pub fn apply_event(&mut self, envelope: &FamilyEvent) {
        let _handled = self.apply_members(envelope)
            || self.apply_tasks(envelope)
            || self.apply_places(envelope)
            || self.apply_problems(envelope);
    }
}
