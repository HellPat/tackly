//! The Tackly domain: the family aggregate with its commands and events
//! (CQRS, via `cqrs-es`), and the state replaying the events gives. No
//! network, no crypto, no disk.

pub mod aggregate;
pub mod events;
pub mod projection;

pub use aggregate::{CommandContext, FamilyCommand, FamilyError, MAX_FOCUS};
pub use events::{
    DomainEvent, FamilyEvent, GeoPoint, MAX_PHOTO_BYTES, Picture, PlaceLocation, Solution,
};
pub use projection::{
    Completion, Family, Member, Opening, Place, PlaceGroup, Problem, Progress, Session, Task,
    TaskList,
};
