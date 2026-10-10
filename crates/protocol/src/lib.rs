//! Types shared by the Tackly app and sync server: the family aggregate with
//! its commands and events (CQRS, via `cqrs-es`), and the HTTP and SSE wire
//! format.

pub mod aggregate;
pub mod events;
pub mod projection;
pub mod wire;

pub use aggregate::{FamilyCommand, FamilyError, Services};
pub use events::{CompletionMetadata, DomainEvent, FamilyEvent, GeoPoint, PlaceLocation};
pub use projection::{
    Activity, ActivityKind, CompletionClaim, Family, Member, Place, PlaceGroup, Task, TaskList,
    TaskStatus,
};
