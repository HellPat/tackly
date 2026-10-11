//! Types shared by the Tackly app and sync server: the family aggregate with
//! its commands and events (CQRS, via `cqrs-es`), and the HTTP and SSE wire
//! format.

pub mod aggregate;
pub mod events;
pub mod projection;
pub mod wire;

pub use aggregate::{CommandContext, FamilyCommand, FamilyError, MAX_FOCUS};
pub use events::{
    DomainEvent, FamilyEvent, GeoPoint, MAX_PHOTO_BYTES, Picture, PlaceLocation, Solution,
};
pub use projection::{
    Completion, Family, Member, Opening, Place, PlaceGroup, Problem, Progress, Session, Task,
    TaskList,
};
