//! What a person can ask of the family.

use chrono::{DateTime, NaiveDate, Utc};
use uuid::Uuid;

use super::events::{Picture, PlaceLocation, Solution};
use crate::values::{Focus, GeoPoint, IconName, Name};

/// A command, by the part of the family it is about. Each area decides its
/// own commands; a new command belongs to one by its type.
#[derive(Clone, Debug)]
pub enum FamilyCommand {
    Members(MemberCommand),
    Tasks(TaskCommand),
    Places(PlaceCommand),
    Problems(ProblemCommand),
}

/// The family and who is in it.
#[derive(Clone, Debug)]
pub enum MemberCommand {
    CreateFamily {
        family_id: Uuid,
        name: Name,
        owner_name: Name,
    },
    Join {
        name: Name,
    },
    Rename {
        name: Name,
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
        name: Name,
        until: DateTime<Utc>,
    },
}

/// Lists and tasks: what to do, who does it, and when it is done.
#[derive(Clone, Debug)]
pub enum TaskCommand {
    CreateList {
        list_id: Uuid,
        name: Name,
    },
    RenameList {
        list_id: Uuid,
        name: Name,
    },
    DeleteList {
        list_id: Uuid,
    },

    /// `list_id: None` puts it in "Other".
    AddTask {
        task_id: Uuid,
        list_id: Option<Uuid>,
        title: Name,
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
}

/// Places, in groups, with their locations.
#[derive(Clone, Debug)]
pub enum PlaceCommand {
    CreatePlaceGroup {
        group_id: Uuid,
        name: Name,
    },
    RenamePlaceGroup {
        group_id: Uuid,
        name: Name,
    },
    DeletePlaceGroup {
        group_id: Uuid,
    },
    CreatePlace {
        place_id: Uuid,
        group_id: Uuid,
        name: Name,
        location: PlaceLocation,
    },
    RenamePlace {
        place_id: Uuid,
        name: Name,
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
}

/// Problems with their solutions, and plans for a day.
#[derive(Clone, Debug)]
pub enum ProblemCommand {
    /// A problem the family knows from now on, with its solutions.
    CreateProblem {
        problem_id: Uuid,
        title: Name,
        icon: IconName,
        solutions: Vec<Solution>,
    },
    RenameProblem {
        problem_id: Uuid,
        title: Name,
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
        task_ids: Focus,
    },
}

impl From<MemberCommand> for FamilyCommand {
    fn from(command: MemberCommand) -> Self {
        Self::Members(command)
    }
}

impl From<TaskCommand> for FamilyCommand {
    fn from(command: TaskCommand) -> Self {
        Self::Tasks(command)
    }
}

impl From<PlaceCommand> for FamilyCommand {
    fn from(command: PlaceCommand) -> Self {
        Self::Places(command)
    }
}

impl From<ProblemCommand> for FamilyCommand {
    fn from(command: ProblemCommand) -> Self {
        Self::Problems(command)
    }
}
