//! The Tackly domain: the family aggregate with its commands and events
//! (CQRS, via `cqrs-es`), and the state replaying the events gives. No
//! network, no crypto, no disk.

pub mod family;

pub use family::{
    aggregate::CommandContext,
    commands::FamilyCommand,
    errors::FamilyError,
    events::{
        DomainEvent, FamilyEvent, GeoPoint, MAX_PHOTO_BYTES, Picture, PlaceLocation, Solution,
    },
    members::Member,
    places::{Place, PlaceGroup},
    problems::{MAX_FOCUS, Opening, Problem},
    state::Family,
    tasks::{Completion, Progress, Session, Task, TaskList},
};
