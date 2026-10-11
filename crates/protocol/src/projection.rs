//! Replays events into the visible family state. Events are applied in server
//! order, followed by this device's not-yet-uploaded events. Replay never
//! fails: an event that no longer fits (a second completion, a start on a done
//! task, a delete that would orphan something) is ignored, so every device
//! that sees the same events in the same order ends up with the same state.

use std::collections::BTreeMap;

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::events::{DomainEvent, FamilyEvent, GeoPoint, Picture, PlaceLocation};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Member {
    pub device_id: Uuid,
    pub name: String,
    pub owner: bool,
    pub picture: Option<Picture>,
}

/// A list people made. "Other" is not one of these: it is where tasks
/// without a list live, so it can never be missing or deleted.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TaskList {
    pub id: Uuid,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlaceGroup {
    pub id: Uuid,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Place {
    pub id: Uuid,
    pub group_id: Uuid,
    pub name: String,
    /// Never empty.
    pub locations: Vec<PlaceLocation>,
}

/// A stretch of work on a task. `end` is `None` while it runs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Session {
    pub by: Uuid,
    pub start: DateTime<Utc>,
    pub end: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Completion {
    pub by: Uuid,
    pub at: DateTime<Utc>,
    /// Where it was finished, if the phone knew.
    pub location: Option<GeoPoint>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Task {
    pub id: Uuid,
    /// `None`: in "Other".
    pub list_id: Option<Uuid>,
    pub title: String,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub place_ids: Vec<Uuid>,
    /// Who has it.
    pub assignee: Option<Uuid>,
    /// Who gave it to them (the assignee themselves when they took it).
    pub assigned_by: Option<Uuid>,
    /// Work since the task was created or last reopened.
    pub sessions: Vec<Session>,
    pub done: Option<Completion>,
}

/// What the person who has a task is doing with it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Progress {
    /// Has it, has not started.
    Picked,
    Working,
    Paused,
}

impl Task {
    pub fn is_done(&self) -> bool {
        self.done.is_some()
    }

    pub fn is_running(&self) -> bool {
        self.sessions.iter().any(|session| session.end.is_none())
    }

    pub fn progress(&self) -> Option<Progress> {
        self.assignee?;
        Some(if self.is_running() {
            Progress::Working
        } else if self.sessions.is_empty() {
            Progress::Picked
        } else {
            Progress::Paused
        })
    }

    /// Time worked in total, and since `day_start` (the start of today).
    pub fn worked(&self, now: DateTime<Utc>, day_start: DateTime<Utc>) -> (Duration, Duration) {
        let (mut total, mut today) = (Duration::zero(), Duration::zero());
        for session in &self.sessions {
            let end = session.end.unwrap_or(now);
            total += end - session.start;
            let from = session.start.max(day_start);
            if end > from {
                today += end - from;
            }
        }
        (total, today)
    }

    fn stop_running(&mut self, at: DateTime<Utc>) {
        for session in &mut self.sessions {
            if session.end.is_none() {
                session.end = Some(at.max(session.start));
            }
        }
    }
}

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

    pub fn member_name(&self, device_id: Uuid) -> &str {
        self.members
            .get(&device_id)
            .map_or("Someone", |member| member.name.as_str())
    }

    pub fn open_tasks(&self) -> impl Iterator<Item = &Task> {
        self.tasks.values().filter(|task| !task.is_done())
    }

    /// Open tasks in a list; `None` is "Other".
    pub fn open_tasks_in(&self, list_id: Option<Uuid>) -> impl Iterator<Item = &Task> {
        self.open_tasks()
            .filter(move |task| task.list_id == list_id)
    }

    /// Open tasks to do at a place.
    pub fn open_tasks_at(&self, place_id: Uuid) -> impl Iterator<Item = &Task> {
        self.open_tasks()
            .filter(move |task| task.place_ids.contains(&place_id))
    }

    pub fn places_in(&self, group_id: Uuid) -> impl Iterator<Item = &Place> {
        self.places
            .values()
            .filter(move |place| place.group_id == group_id)
    }

    pub fn task_by_title(&self, title: &str) -> Option<&Task> {
        self.tasks.values().find(|task| task.title == title)
    }

    /// Applies one event. Events that do not fit the current state are
    /// ignored (see the module documentation).
    pub fn apply_event(&mut self, envelope: &FamilyEvent) {
        let (subject, by, at) = (
            envelope.subject_id,
            envelope.origin_device_id,
            envelope.occurred_at,
        );
        match &envelope.event {
            DomainEvent::FamilyCreated { name, owner_name } => {
                if self.family_id.is_some() {
                    return;
                }
                self.family_id = Some(subject);
                self.name = Some(name.clone());
                self.members.insert(
                    by,
                    Member {
                        device_id: by,
                        name: owner_name.clone(),
                        owner: true,
                        picture: None,
                    },
                );
            }
            DomainEvent::MemberJoined { name } => {
                let member = self.members.entry(by).or_insert(Member {
                    device_id: by,
                    name: String::new(),
                    owner: false,
                    picture: None,
                });
                member.name = name.clone();
            }
            DomainEvent::MemberRenamed { name } => {
                if let Some(member) = self.members.get_mut(&by) {
                    member.name = name.clone();
                }
            }
            DomainEvent::MemberPictureChanged { picture } => {
                if let Some(member) = self.members.get_mut(&by) {
                    member.picture = Some(picture.clone());
                }
            }

            DomainEvent::ListCreated { name } => {
                self.lists.entry(subject).or_insert(TaskList {
                    id: subject,
                    name: name.clone(),
                });
            }
            DomainEvent::ListRenamed { name } => {
                if let Some(list) = self.lists.get_mut(&subject) {
                    list.name = name.clone();
                }
            }
            DomainEvent::ListDeleted => {
                if self.open_tasks_in(Some(subject)).next().is_none() {
                    self.lists.remove(&subject);
                }
            }

            DomainEvent::PlaceGroupCreated { name } => {
                self.place_groups.entry(subject).or_insert(PlaceGroup {
                    id: subject,
                    name: name.clone(),
                });
            }
            DomainEvent::PlaceGroupRenamed { name } => {
                if let Some(group) = self.place_groups.get_mut(&subject) {
                    group.name = name.clone();
                }
            }
            DomainEvent::PlaceGroupDeleted => {
                if self.places_in(subject).next().is_none() {
                    self.place_groups.remove(&subject);
                }
            }
            DomainEvent::PlaceCreated {
                group_id,
                name,
                location,
            } => {
                if self.place_groups.contains_key(group_id) {
                    self.places.entry(subject).or_insert(Place {
                        id: subject,
                        group_id: *group_id,
                        name: name.clone(),
                        locations: vec![location.clone()],
                    });
                }
            }
            DomainEvent::PlaceRenamed { name } => {
                if let Some(place) = self.places.get_mut(&subject) {
                    place.name = name.clone();
                }
            }
            DomainEvent::PlaceMoved { group_id } => {
                if self.place_groups.contains_key(group_id)
                    && let Some(place) = self.places.get_mut(&subject)
                {
                    place.group_id = *group_id;
                }
            }
            DomainEvent::PlaceDeleted => {
                if self.places.remove(&subject).is_some() {
                    for task in self.tasks.values_mut() {
                        task.place_ids.retain(|id| *id != subject);
                    }
                }
            }
            DomainEvent::PlaceLocationAdded { location } => {
                if let Some(place) = self.places.get_mut(&subject)
                    && !place.locations.iter().any(|known| known.id == location.id)
                {
                    place.locations.push(location.clone());
                }
            }
            DomainEvent::PlaceLocationRemoved { location_id } => {
                if let Some(place) = self.places.get_mut(&subject)
                    && place.locations.len() > 1
                {
                    place
                        .locations
                        .retain(|location| location.id != *location_id);
                }
            }

            DomainEvent::TaskCreated {
                list_id,
                title,
                place_ids,
            } => {
                if self.tasks.contains_key(&subject) {
                    return;
                }
                let list_id = list_id.filter(|id| self.lists.contains_key(id));
                let place_ids = place_ids
                    .iter()
                    .filter(|id| self.places.contains_key(id))
                    .copied()
                    .collect();
                self.tasks.insert(
                    subject,
                    Task {
                        id: subject,
                        list_id,
                        title: title.clone(),
                        created_by: by,
                        created_at: at,
                        place_ids,
                        assignee: None,
                        assigned_by: None,
                        sessions: Vec::new(),
                        done: None,
                    },
                );
            }
            DomainEvent::TaskDeleted => {
                self.tasks.remove(&subject);
            }
            DomainEvent::TaskPlaceAdded { place_id } => {
                if !self.places.contains_key(place_id) {
                    return;
                }
                if let Some(task) = self.tasks.get_mut(&subject)
                    && !task.place_ids.contains(place_id)
                {
                    task.place_ids.push(*place_id);
                }
            }
            DomainEvent::TaskPlaceRemoved { place_id } => {
                if let Some(task) = self.tasks.get_mut(&subject) {
                    task.place_ids.retain(|id| id != place_id);
                }
            }
            DomainEvent::TaskAssigned { member_id } => {
                let known = member_id.is_none_or(|id| self.members.contains_key(&id));
                if let Some(task) = self.tasks.get_mut(&subject)
                    && !task.is_done()
                    && known
                {
                    if task.assignee != *member_id {
                        task.stop_running(at);
                    }
                    task.assignee = *member_id;
                    task.assigned_by = member_id.map(|_| by);
                }
            }
            DomainEvent::TaskStarted => {
                if let Some(task) = self.tasks.get_mut(&subject)
                    && !task.is_done()
                {
                    if task.assignee != Some(by) {
                        task.stop_running(at);
                        task.assignee = Some(by);
                        task.assigned_by = Some(by);
                    }
                    if !task.is_running() {
                        task.sessions.push(Session {
                            by,
                            start: at,
                            end: None,
                        });
                    }
                }
            }
            DomainEvent::TaskPaused => {
                if let Some(task) = self.tasks.get_mut(&subject) {
                    task.stop_running(at);
                }
            }
            DomainEvent::TaskCompleted { location } => {
                // A second completion changes nothing: done is done. Only a
                // session the second finisher left running is closed.
                if let Some(task) = self.tasks.get_mut(&subject) {
                    task.stop_running(at);
                    if task.done.is_none() {
                        task.done = Some(Completion {
                            by,
                            at,
                            location: *location,
                        });
                    }
                }
            }
            DomainEvent::TaskCompletionLocated { location } => {
                if let Some(Completion {
                    by: finisher,
                    location: where_done @ None,
                    ..
                }) = self
                    .tasks
                    .get_mut(&subject)
                    .and_then(|task| task.done.as_mut())
                    && *finisher == by
                {
                    *where_done = Some(*location);
                }
            }
            DomainEvent::TaskCompletionUndone => {
                if let Some(task) = self.tasks.get_mut(&subject) {
                    task.done = None;
                }
            }
            DomainEvent::TaskReopened => {
                if let Some(task) = self.tasks.get_mut(&subject)
                    && task.is_done()
                {
                    task.done = None;
                    task.sessions.clear();
                    task.assignee = None;
                    task.assigned_by = None;
                }
            }
            DomainEvent::Unknown => {}
        }
    }
}
