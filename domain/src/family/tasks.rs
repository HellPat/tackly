//! Task lists and tasks: who has a task, the work on it, finishing, undoing.

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::values::GeoPoint;

use super::{
    commands::TaskCommand,
    errors::FamilyError,
    events::{DomainEvent, FamilyEvent},
    state::Family,
};

/// A list people made. "Other" is not one of these: it is where tasks
/// without a list live, so it can never be missing or deleted.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TaskList {
    pub id: Uuid,
    pub name: String,
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
    /// A step of this problem.
    pub problem_id: Option<Uuid>,
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
            // A session that ended before today adds nothing to today.
            today += (end - session.start.max(day_start)).max(Duration::zero());
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

impl Family {
    pub fn open_tasks(&self) -> impl Iterator<Item = &Task> {
        self.tasks.values().filter(|task| !task.is_done())
    }

    /// Open tasks in a list; `None` is "Other". Steps of problems are not in
    /// any list: they live with their problem.
    pub fn open_tasks_in(&self, list_id: Option<Uuid>) -> impl Iterator<Item = &Task> {
        self.open_tasks()
            .filter(move |task| task.list_id == list_id && task.problem_id.is_none())
    }

    pub fn task_by_title(&self, title: &str) -> Option<&Task> {
        self.tasks.values().find(|task| task.title == title)
    }

    pub(super) fn open_task(&self, task_id: Uuid) -> Result<&Task, FamilyError> {
        let task = self.tasks.get(&task_id).ok_or(FamilyError::UnknownTask)?;
        if task.is_done() {
            return Err(FamilyError::AlreadyDone);
        }
        Ok(task)
    }

    /// A task that is free, or mine.
    pub(super) fn my_open_task(&self, task_id: Uuid, me: Uuid) -> Result<&Task, FamilyError> {
        let task = self.open_task(task_id)?;
        match task.assignee {
            Some(someone) if someone != me => Err(FamilyError::Taken),
            _ => Ok(task),
        }
    }

    /// Checks the tasks commands and returns the events they produce, as (subject, event).
    pub(super) fn decide_tasks(
        &self,
        command: TaskCommand,
        me: Uuid,
    ) -> Result<Vec<(Uuid, DomainEvent)>, FamilyError> {
        use DomainEvent as E;
        use TaskCommand as C;
        let list = |id: Uuid| self.lists.get(&id).ok_or(FamilyError::UnknownList);
        let place = |id: Uuid| self.places.get(&id).ok_or(FamilyError::UnknownPlace);
        let one = |subject, event| Ok(vec![(subject, event)]);
        match command {
            C::CreateList { list_id, name } => one(list_id, E::ListCreated { name }),
            C::RenameList { list_id, name } => {
                list(list_id)?;
                one(list_id, E::ListRenamed { name })
            }
            C::DeleteList { list_id } => {
                list(list_id)?;
                if self.open_tasks_in(Some(list_id)).next().is_some() {
                    return Err(FamilyError::ListNotEmpty);
                }
                one(list_id, E::ListDeleted)
            }
            C::AddTask {
                task_id,
                list_id,
                title,
                place_ids,
            } => {
                if let Some(id) = list_id {
                    list(id)?;
                }
                for id in &place_ids {
                    place(*id)?;
                }
                one(
                    task_id,
                    E::TaskCreated {
                        list_id,
                        title,
                        place_ids,
                        problem_id: None,
                    },
                )
            }
            C::DeleteTask { task_id } => {
                self.tasks.get(&task_id).ok_or(FamilyError::UnknownTask)?;
                one(task_id, E::TaskDeleted)
            }
            C::AddTaskToPlace { task_id, place_id } => {
                let task = self.tasks.get(&task_id).ok_or(FamilyError::UnknownTask)?;
                place(place_id)?;
                Ok(if task.place_ids.contains(&place_id) {
                    vec![]
                } else {
                    vec![(task_id, E::TaskPlaceAdded { place_id })]
                })
            }
            C::RemoveTaskFromPlace { task_id, place_id } => {
                let task = self.tasks.get(&task_id).ok_or(FamilyError::UnknownTask)?;
                Ok(if task.place_ids.contains(&place_id) {
                    vec![(task_id, E::TaskPlaceRemoved { place_id })]
                } else {
                    vec![]
                })
            }
            C::AssignTask { task_id, member_id } => {
                let task = self.open_task(task_id)?;
                if let Some(id) = member_id {
                    self.members.get(&id).ok_or(FamilyError::UnknownMember)?;
                }
                // Free tasks can be given to anyone; a given task can be changed
                // by who has it or who gave it (that is also how Undo works).
                let allowed = task.assignee.is_none()
                    || task.assignee == Some(me)
                    || task.assigned_by == Some(me);
                if !allowed {
                    return Err(FamilyError::Taken);
                }
                one(task_id, E::TaskAssigned { member_id })
            }
            C::StartTask { task_id } => {
                let task = self.my_open_task(task_id, me)?;
                Ok(if task.is_running() {
                    vec![]
                } else {
                    vec![(task_id, E::TaskStarted)]
                })
            }
            C::PauseTask { task_id } => {
                let task = self.my_open_task(task_id, me)?;
                if !task.is_running() {
                    return Err(FamilyError::NotWorking);
                }
                one(task_id, E::TaskPaused)
            }
            C::CompleteTask { task_id, location } => {
                self.my_open_task(task_id, me)?;
                one(task_id, E::TaskCompleted { location })
            }
            C::LocateCompletion { task_id, location } => {
                let task = self.tasks.get(&task_id).ok_or(FamilyError::UnknownTask)?;
                match &task.done {
                    None => Err(FamilyError::NotDone),
                    Some(done) if done.by != me => Err(FamilyError::NotYours),
                    Some(done) if done.location.is_some() => Err(FamilyError::AlreadyLocated),
                    Some(_) => one(task_id, E::TaskCompletionLocated { location }),
                }
            }
            C::UndoCompletion { task_id } => {
                let task = self.tasks.get(&task_id).ok_or(FamilyError::UnknownTask)?;
                match &task.done {
                    None => Err(FamilyError::NotDone),
                    Some(done) if done.by != me => Err(FamilyError::NotYours),
                    Some(_) => one(task_id, E::TaskCompletionUndone),
                }
            }
            C::ReopenTask { task_id } => {
                let task = self.tasks.get(&task_id).ok_or(FamilyError::UnknownTask)?;
                if !task.is_done() {
                    return Err(FamilyError::NotDone);
                }
                one(task_id, E::TaskReopened)
            }
        }
    }

    /// Applies the tasks events; `false` when the event is not one of them.
    pub(super) fn apply_tasks(&mut self, envelope: &FamilyEvent) -> bool {
        let (subject, by, at) = (
            envelope.subject_id,
            envelope.origin_device_id,
            envelope.occurred_at,
        );
        match &envelope.event {
            DomainEvent::ListCreated { name } => {
                self.lists.entry(subject).or_insert(TaskList {
                    id: subject,
                    name: name.to_string(),
                });
            }
            DomainEvent::ListRenamed { name } => {
                if let Some(list) = self.lists.get_mut(&subject) {
                    list.name = name.to_string();
                }
            }
            DomainEvent::ListDeleted => {
                if self.open_tasks_in(Some(subject)).next().is_none() {
                    self.lists.remove(&subject);
                }
            }
            DomainEvent::TaskCreated {
                list_id,
                title,
                place_ids,
                problem_id,
            } => {
                if self.tasks.contains_key(&subject) {
                    return true;
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
                        title: title.to_string(),
                        created_by: by,
                        created_at: at,
                        place_ids,
                        assignee: None,
                        assigned_by: None,
                        sessions: Vec::new(),
                        done: None,
                        problem_id: *problem_id,
                    },
                );
            }
            DomainEvent::TaskDeleted => {
                self.tasks.remove(&subject);
            }
            DomainEvent::TaskPlaceAdded { place_id } => {
                if !self.places.contains_key(place_id) {
                    return true;
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
            _ => return false,
        }
        true
    }
}
