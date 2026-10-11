//! Why the family refuses a command.

use super::problems::MAX_FOCUS;

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
