//! The Tackly domain: the family aggregate with its commands and events
//! (CQRS, via `cqrs-es`), and the state replaying the events gives. No
//! network, no crypto, no disk.

pub mod family;
pub mod values;

pub use family::{
    aggregate::CommandContext,
    commands::{FamilyCommand, MemberCommand, PlaceCommand, ProblemCommand, TaskCommand},
    errors::FamilyError,
    events::{DomainEvent, FamilyEvent, Picture, PlaceLocation, Solution},
    members::Member,
    places::{Place, PlaceGroup},
    problems::{Opening, Problem},
    state::Family,
    tasks::{Completion, Progress, Session, Task, TaskList},
};
pub use values::{
    EmptyName, Focus, GeoPoint, GeoPointError, IconName, MAX_FOCUS, MAX_PHOTO_BYTES, Name,
    NotAnIconName, Photo, PhotoError, TooMuch,
};
