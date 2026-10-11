//! The family aggregate: commands are checked against the replayed state and
//! produce events. `Family::apply_event` stays lenient, because events from
//! other devices arrive in server order and may overlap.

use std::sync::Arc;

use chrono::{DateTime, NaiveDate, Utc};
use cqrs_es::{Aggregate, event_sink::EventSink};
use uuid::Uuid;

use crate::{DomainEvent, Family, FamilyEvent, GeoPoint, Picture, PlaceLocation, Solution};

#[derive(Clone, Debug)]
pub enum FamilyCommand {
    CreateFamily {
        family_id: Uuid,
        name: String,
        owner_name: String,
    },
    Join {
        name: String,
    },
    Rename {
        name: String,
    },
    SetPicture {
        picture: Picture,
    },
    /// The head takes someone out of the family (a member or an AI agent).
    RemoveMember {
        member_id: Uuid,
    },
    /// The head lets an AI agent read along until `until`.
    AdmitAgent {
        member_id: Uuid,
        name: String,
        until: DateTime<Utc>,
    },

    CreateList {
        list_id: Uuid,
        name: String,
    },
    RenameList {
        list_id: Uuid,
        name: String,
    },
    DeleteList {
        list_id: Uuid,
    },

    CreatePlaceGroup {
        group_id: Uuid,
        name: String,
    },
    RenamePlaceGroup {
        group_id: Uuid,
        name: String,
    },
    DeletePlaceGroup {
        group_id: Uuid,
    },
    CreatePlace {
        place_id: Uuid,
        group_id: Uuid,
        name: String,
        location: PlaceLocation,
    },
    RenamePlace {
        place_id: Uuid,
        name: String,
    },
    MovePlace {
        place_id: Uuid,
        group_id: Uuid,
    },
    DeletePlace {
        place_id: Uuid,
    },
    AddPlaceLocation {
        place_id: Uuid,
        location: PlaceLocation,
    },
    RemovePlaceLocation {
        place_id: Uuid,
        location_id: Uuid,
    },

    /// `list_id: None` puts it in "Other".
    AddTask {
        task_id: Uuid,
        list_id: Option<Uuid>,
        title: String,
        place_ids: Vec<Uuid>,
    },
    DeleteTask {
        task_id: Uuid,
    },
    AddTaskToPlace {
        task_id: Uuid,
        place_id: Uuid,
    },
    RemoveTaskFromPlace {
        task_id: Uuid,
        place_id: Uuid,
    },
    /// Take a task, give it to someone, or (`None`) give it back.
    AssignTask {
        task_id: Uuid,
        member_id: Option<Uuid>,
    },
    StartTask {
        task_id: Uuid,
    },
    PauseTask {
        task_id: Uuid,
    },
    CompleteTask {
        task_id: Uuid,
        /// Where the phone is, if it can tell.
        location: Option<GeoPoint>,
    },
    /// Where the phone was when it finished the task, found out afterwards.
    LocateCompletion {
        task_id: Uuid,
        location: GeoPoint,
    },
    UndoCompletion {
        task_id: Uuid,
    },
    ReopenTask {
        task_id: Uuid,
    },

    /// A problem the family knows from now on, with its solutions.
    CreateProblem {
        problem_id: Uuid,
        title: String,
        icon: String,
        solutions: Vec<Solution>,
    },
    RenameProblem {
        problem_id: Uuid,
        title: String,
    },
    /// While the problem is open, the new solution is a step right away: `task_id`.
    AddSolution {
        problem_id: Uuid,
        solution: Solution,
        task_id: Uuid,
    },
    RemoveSolution {
        problem_id: Uuid,
        solution_id: Uuid,
    },
    /// Noticed for yourself, or raised for someone else. One new task per
    /// solution, in order: `task_ids`.
    NoticeProblem {
        problem_id: Uuid,
        member_id: Uuid,
        task_ids: Vec<Uuid>,
    },
    /// Someone else has it from now on, with its open steps.
    AssignProblem {
        problem_id: Uuid,
        member_id: Uuid,
    },
    ResolveProblem {
        problem_id: Uuid,
    },
    /// Undo of resolving.
    ReopenProblem {
        problem_id: Uuid,
    },

    /// My focus for a day, in order. Free tasks become mine.
    PlanDay {
        day: NaiveDate,
        task_ids: Vec<Uuid>,
    },
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum FamilyError {
    #[error("this phone is already in a family")]
    AlreadyCreated,
    #[error("not in a family yet")]
    NoFamily,
    #[error("names and titles must not be empty")]
    Empty,
    #[error("unknown member")]
    UnknownMember,
    #[error("only the head of the family can do that")]
    NotHead,
    #[error("the head cannot be removed")]
    HeadStays,
    #[error("unknown list")]
    UnknownList,
    #[error("a list with open tasks can't be deleted")]
    ListNotEmpty,
    #[error("unknown place group")]
    UnknownGroup,
    #[error("a group with places can't be deleted")]
    GroupNotEmpty,
    #[error("unknown place")]
    UnknownPlace,
    #[error("a place keeps at least one location")]
    LastLocation,
    #[error("a photo must be a small JPEG")]
    BadPhoto,
    #[error("unknown task")]
    UnknownTask,
    #[error("the task is already done")]
    AlreadyDone,
    #[error("the task is not done")]
    NotDone,
    #[error("where the task was finished is already known")]
    AlreadyLocated,
    #[error("someone else has this task")]
    Taken,
    #[error("you are not working on it")]
    NotWorking,
    #[error("only who finished it can undo that")]
    NotYours,
    #[error("unknown problem")]
    UnknownProblem,
    #[error("the problem is already open")]
    ProblemOpen,
    #[error("the problem is not open")]
    ProblemNotOpen,
    #[error("one new task per solution")]
    WrongSteps,
    #[error("a day holds at most {MAX_FOCUS} tasks")]
    TooMuch,
}

/// The most tasks one day's plan may hold. The app asks for fewer (3 by default).
pub const MAX_FOCUS: usize = 10;

/// Who is acting, what time it is, and where new event IDs come from. On a
/// phone: the system clock and random IDs. In tests: fixed, so expected events
/// can be written out exactly.
#[derive(Clone)]
pub struct CommandContext {
    pub device_id: Uuid,
    clock: Arc<dyn Fn() -> DateTime<Utc> + Send + Sync>,
    event_ids: Arc<dyn Fn() -> Uuid + Send + Sync>,
}

impl CommandContext {
    /// On a phone: this device, the system clock, random event IDs.
    pub fn for_device(device_id: Uuid) -> Self {
        Self {
            device_id,
            clock: Arc::new(Utc::now),
            // Random, not time-ordered: the server sees event IDs.
            event_ids: Arc::new(Uuid::new_v4),
        }
    }

    /// Always the same time and event ID; for tests.
    pub fn fixed(device_id: Uuid, at: DateTime<Utc>, event_id: Uuid) -> Self {
        Self {
            device_id,
            clock: Arc::new(move || at),
            event_ids: Arc::new(move || event_id),
        }
    }
}

impl std::fmt::Debug for CommandContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CommandContext")
            .field("device_id", &self.device_id)
            .finish()
    }
}

fn not_empty(text: &str) -> Result<String, FamilyError> {
    let text = text.trim();
    if text.is_empty() {
        Err(FamilyError::Empty)
    } else {
        Ok(text.to_owned())
    }
}

impl Family {
    fn open_task(&self, task_id: Uuid) -> Result<&crate::Task, FamilyError> {
        let task = self.tasks.get(&task_id).ok_or(FamilyError::UnknownTask)?;
        if task.is_done() {
            return Err(FamilyError::AlreadyDone);
        }
        Ok(task)
    }

    /// A task that is free, or mine.
    fn my_open_task(&self, task_id: Uuid, me: Uuid) -> Result<&crate::Task, FamilyError> {
        let task = self.open_task(task_id)?;
        match task.assignee {
            Some(someone) if someone != me => Err(FamilyError::Taken),
            _ => Ok(task),
        }
    }

    /// Checks a command and returns the events it produces, as (subject, event).
    fn decide(
        &self,
        command: FamilyCommand,
        me: Uuid,
    ) -> Result<Vec<(Uuid, DomainEvent)>, FamilyError> {
        use DomainEvent as E;
        use FamilyCommand as C;
        let family = || self.family_id.ok_or(FamilyError::NoFamily);
        let list = |id: Uuid| self.lists.get(&id).ok_or(FamilyError::UnknownList);
        let group = |id: Uuid| self.place_groups.get(&id).ok_or(FamilyError::UnknownGroup);
        let place = |id: Uuid| self.places.get(&id).ok_or(FamilyError::UnknownPlace);
        let one = |subject, event| Ok(vec![(subject, event)]);
        match command {
            C::CreateFamily {
                family_id,
                name,
                owner_name,
            } => {
                if self.family_id.is_some() {
                    return Err(FamilyError::AlreadyCreated);
                }
                one(
                    family_id,
                    E::FamilyCreated {
                        name: not_empty(&name)?,
                        owner_name: not_empty(&owner_name)?,
                    },
                )
            }
            C::Join { name } => one(
                family()?,
                E::MemberJoined {
                    name: not_empty(&name)?,
                },
            ),
            C::Rename { name } => one(
                family()?,
                E::MemberRenamed {
                    name: not_empty(&name)?,
                },
            ),
            C::SetPicture { picture } => {
                if !picture.photo_is_valid() {
                    return Err(FamilyError::BadPhoto);
                }
                one(family()?, E::MemberPictureChanged { picture })
            }
            C::RemoveMember { member_id } => {
                if !self.is_head(me) {
                    return Err(FamilyError::NotHead);
                }
                if member_id == me {
                    return Err(FamilyError::HeadStays);
                }
                self.members
                    .get(&member_id)
                    .ok_or(FamilyError::UnknownMember)?;
                one(family()?, E::MemberRemoved { member_id })
            }
            C::AdmitAgent {
                member_id,
                name,
                until,
            } => {
                if !self.is_head(me) {
                    return Err(FamilyError::NotHead);
                }
                if member_id == me {
                    return Err(FamilyError::HeadStays);
                }
                one(
                    family()?,
                    E::AgentAdmitted {
                        member_id,
                        name: not_empty(&name)?,
                        until,
                    },
                )
            }

            C::CreateList { list_id, name } => one(
                list_id,
                E::ListCreated {
                    name: not_empty(&name)?,
                },
            ),
            C::RenameList { list_id, name } => {
                list(list_id)?;
                one(
                    list_id,
                    E::ListRenamed {
                        name: not_empty(&name)?,
                    },
                )
            }
            C::DeleteList { list_id } => {
                list(list_id)?;
                if self.open_tasks_in(Some(list_id)).next().is_some() {
                    return Err(FamilyError::ListNotEmpty);
                }
                one(list_id, E::ListDeleted)
            }

            C::CreatePlaceGroup { group_id, name } => one(
                group_id,
                E::PlaceGroupCreated {
                    name: not_empty(&name)?,
                },
            ),
            C::RenamePlaceGroup { group_id, name } => {
                group(group_id)?;
                one(
                    group_id,
                    E::PlaceGroupRenamed {
                        name: not_empty(&name)?,
                    },
                )
            }
            C::DeletePlaceGroup { group_id } => {
                group(group_id)?;
                if self.places_in(group_id).next().is_some() {
                    return Err(FamilyError::GroupNotEmpty);
                }
                one(group_id, E::PlaceGroupDeleted)
            }
            C::CreatePlace {
                place_id,
                group_id,
                name,
                mut location,
            } => {
                group(group_id)?;
                location.name = not_empty(&location.name)?;
                one(
                    place_id,
                    E::PlaceCreated {
                        group_id,
                        name: not_empty(&name)?,
                        location,
                    },
                )
            }
            C::RenamePlace { place_id, name } => {
                place(place_id)?;
                one(
                    place_id,
                    E::PlaceRenamed {
                        name: not_empty(&name)?,
                    },
                )
            }
            C::MovePlace { place_id, group_id } => {
                place(place_id)?;
                group(group_id)?;
                one(place_id, E::PlaceMoved { group_id })
            }
            C::DeletePlace { place_id } => {
                place(place_id)?;
                one(place_id, E::PlaceDeleted)
            }
            C::AddPlaceLocation {
                place_id,
                mut location,
            } => {
                place(place_id)?;
                location.name = not_empty(&location.name)?;
                one(place_id, E::PlaceLocationAdded { location })
            }
            C::RemovePlaceLocation {
                place_id,
                location_id,
            } => {
                if place(place_id)?.locations.len() <= 1 {
                    return Err(FamilyError::LastLocation);
                }
                one(place_id, E::PlaceLocationRemoved { location_id })
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
                        title: not_empty(&title)?,
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

            C::CreateProblem {
                problem_id,
                title,
                icon,
                solutions,
            } => {
                let mut events = vec![(
                    problem_id,
                    E::ProblemCreated {
                        title: not_empty(&title)?,
                        icon,
                    },
                )];
                for mut solution in solutions {
                    solution.title = not_empty(&solution.title)?;
                    events.push((problem_id, E::ProblemSolutionAdded { solution }));
                }
                Ok(events)
            }
            C::RenameProblem { problem_id, title } => {
                self.problem(problem_id)?;
                one(
                    problem_id,
                    E::ProblemRenamed {
                        title: not_empty(&title)?,
                    },
                )
            }
            C::AddSolution {
                problem_id,
                mut solution,
                task_id,
            } => {
                let problem = self.problem(problem_id)?;
                solution.title = not_empty(&solution.title)?;
                let mut events = vec![];
                if let Some(holder) = problem.holder() {
                    events.extend(step(problem_id, &solution, task_id, holder));
                }
                events.insert(0, (problem_id, E::ProblemSolutionAdded { solution }));
                Ok(events)
            }
            C::RemoveSolution {
                problem_id,
                solution_id,
            } => {
                self.problem(problem_id)?;
                one(problem_id, E::ProblemSolutionRemoved { solution_id })
            }
            C::NoticeProblem {
                problem_id,
                member_id,
                task_ids,
            } => {
                let problem = self.problem(problem_id)?;
                if problem.open {
                    return Err(FamilyError::ProblemOpen);
                }
                self.members
                    .get(&member_id)
                    .ok_or(FamilyError::UnknownMember)?;
                if task_ids.len() != problem.solutions.len() {
                    return Err(FamilyError::WrongSteps);
                }
                let mut events = vec![(problem_id, E::ProblemNoticed { member_id })];
                for (solution, task_id) in problem.solutions.iter().zip(task_ids) {
                    events.extend(step(problem_id, solution, task_id, member_id));
                }
                Ok(events)
            }
            C::AssignProblem {
                problem_id,
                member_id,
            } => {
                if !self.problem(problem_id)?.open {
                    return Err(FamilyError::ProblemNotOpen);
                }
                self.members
                    .get(&member_id)
                    .ok_or(FamilyError::UnknownMember)?;
                let mut events = vec![(problem_id, E::ProblemAssigned { member_id })];
                for task in self.steps(problem_id) {
                    if task.assignee != Some(member_id) {
                        events.push((
                            task.id,
                            E::TaskAssigned {
                                member_id: Some(member_id),
                            },
                        ));
                    }
                }
                Ok(events)
            }
            C::ResolveProblem { problem_id } => {
                if !self.problem(problem_id)?.open {
                    return Err(FamilyError::ProblemNotOpen);
                }
                one(problem_id, E::ProblemResolved)
            }
            C::ReopenProblem { problem_id } => {
                let problem = self.problem(problem_id)?;
                if problem.open {
                    return Err(FamilyError::ProblemOpen);
                }
                if problem.opening.is_none() {
                    return Err(FamilyError::ProblemNotOpen);
                }
                one(problem_id, E::ProblemReopened)
            }

            C::PlanDay { day, task_ids } => {
                let mut planned: Vec<Uuid> = Vec::new();
                for id in task_ids {
                    if !planned.contains(&id) {
                        planned.push(id);
                    }
                }
                if planned.len() > MAX_FOCUS {
                    return Err(FamilyError::TooMuch);
                }
                let mut events = vec![];
                for id in &planned {
                    let task = self.my_open_task(*id, me)?;
                    if task.assignee.is_none() {
                        events.push((
                            *id,
                            E::TaskAssigned {
                                member_id: Some(me),
                            },
                        ));
                    }
                }
                events.push((
                    me,
                    E::DayPlanned {
                        day,
                        task_ids: planned,
                    },
                ));
                Ok(events)
            }
        }
    }

    fn problem(&self, problem_id: Uuid) -> Result<&crate::Problem, FamilyError> {
        self.problems
            .get(&problem_id)
            .ok_or(FamilyError::UnknownProblem)
    }
}

/// A solution becomes a task of its own, for whoever has the problem.
fn step(
    problem_id: Uuid,
    solution: &Solution,
    task_id: Uuid,
    holder: Uuid,
) -> [(Uuid, DomainEvent); 2] {
    [
        (
            task_id,
            DomainEvent::TaskCreated {
                list_id: None,
                title: solution.title.clone(),
                place_ids: vec![],
                problem_id: Some(problem_id),
            },
        ),
        (
            task_id,
            DomainEvent::TaskAssigned {
                member_id: Some(holder),
            },
        ),
    ]
}

impl Aggregate for Family {
    const TYPE: &'static str = "family";
    type Command = FamilyCommand;
    type Event = FamilyEvent;
    type Error = FamilyError;
    /// cqrs-es calls what a command gets from outside "services".
    type Services = CommandContext;

    async fn handle(
        &mut self,
        command: FamilyCommand,
        context: &CommandContext,
        sink: &EventSink<Self>,
    ) -> Result<(), FamilyError> {
        for (subject_id, event) in self.decide(command, context.device_id)? {
            let envelope = FamilyEvent {
                id: (context.event_ids)(),
                subject_id,
                origin_device_id: context.device_id,
                occurred_at: (context.clock)(),
                event,
            };
            sink.write(envelope, self).await;
        }
        Ok(())
    }

    fn apply(&mut self, event: FamilyEvent) {
        self.apply_event(&event);
    }
}
