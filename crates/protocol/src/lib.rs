//! Types shared by the Tackly app and sync server: the family aggregate with
//! its commands and events (CQRS, via `cqrs-es`), and the HTTP and SSE wire
//! format.

pub mod aggregate;
pub mod events;
pub mod projection;
pub mod wire;

pub use aggregate::{CommandContext, FamilyCommand, FamilyError};
pub use events::{DomainEvent, FamilyEvent, GeoPoint, Picture, PlaceLocation};
pub use projection::{
    Completion, Family, Member, Place, PlaceGroup, Progress, Session, Task, TaskList,
};
