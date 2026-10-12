//! What a person can ask of the family.

use chrono::{DateTime, NaiveDate, Utc};
use uuid::Uuid;

use super::events::{Picture, PlaceLocation, Solution};
use crate::values::{Focus, GeoPoint, Name};

#[derive(Clone, Debug)]
pub enum FamilyCommand {
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

    /// A problem the family knows from now on, with its solutions.
    CreateProblem {
        problem_id: Uuid,
        title: Name,
        icon: String,
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

/// The part of the family a command is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Area {
    Members,
    Tasks,
    Places,
    Problems,
}

impl FamilyCommand {
    /// Every command belongs to exactly one area; adding one fails to compile
    /// until it is placed here.
    pub(super) fn area(&self) -> Area {
        use FamilyCommand as C;
        match self {
            C::CreateFamily { .. }
            | C::Join { .. }
            | C::Rename { .. }
            | C::SetPicture { .. }
            | C::RemoveMember { .. }
            | C::AdmitAgent { .. } => Area::Members,
            C::CreateList { .. }
            | C::RenameList { .. }
            | C::DeleteList { .. }
            | C::AddTask { .. }
            | C::DeleteTask { .. }
            | C::AddTaskToPlace { .. }
            | C::RemoveTaskFromPlace { .. }
            | C::AssignTask { .. }
            | C::StartTask { .. }
            | C::PauseTask { .. }
            | C::CompleteTask { .. }
            | C::LocateCompletion { .. }
            | C::UndoCompletion { .. }
            | C::ReopenTask { .. } => Area::Tasks,
            C::CreatePlaceGroup { .. }
            | C::RenamePlaceGroup { .. }
            | C::DeletePlaceGroup { .. }
            | C::CreatePlace { .. }
            | C::RenamePlace { .. }
            | C::MovePlace { .. }
            | C::DeletePlace { .. }
            | C::AddPlaceLocation { .. }
            | C::RemovePlaceLocation { .. } => Area::Places,
            C::CreateProblem { .. }
            | C::RenameProblem { .. }
            | C::AddSolution { .. }
            | C::RemoveSolution { .. }
            | C::NoticeProblem { .. }
            | C::AssignProblem { .. }
            | C::ResolveProblem { .. }
            | C::ReopenProblem { .. }
            | C::PlanDay { .. } => Area::Problems,
        }
    }
}
