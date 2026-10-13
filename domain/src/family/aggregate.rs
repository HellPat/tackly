//! The family aggregate (CQRS, via `cqrs-es`): commands are checked against the
//! replayed state and produce events. Each area of the family decides its own
//! commands; this file routes them and writes the events.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use cqrs_es::{Aggregate, event_sink::EventSink};
use uuid::Uuid;

use super::{
    commands::FamilyCommand,
    errors::FamilyError,
    events::{DomainEvent, FamilyEvent},
    state::Family,
};

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

impl Family {
    /// Checks a command and returns the events it produces, as (subject, event).
    fn decide(
        &self,
        command: FamilyCommand,
        me: Uuid,
    ) -> Result<Vec<(Uuid, DomainEvent)>, FamilyError> {
        match command {
            FamilyCommand::Members(command) => self.decide_members(command, me),
            FamilyCommand::Tasks(command) => self.decide_tasks(command, me),
            FamilyCommand::Places(command) => self.decide_places(command, me),
            FamilyCommand::Problems(command) => self.decide_problems(command, me),
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn on_a_phone_events_get_the_clock_and_fresh_ids() {
        let device = Uuid::from_u128(1);
        let context = CommandContext::for_device(device);
        assert_eq!(context.device_id, device);
        assert_ne!((context.event_ids)(), (context.event_ids)());
        let before = Utc::now();
        let at = (context.clock)();
        assert!(at >= before && at <= Utc::now());
        assert_eq!(
            format!("{context:?}"),
            format!("CommandContext {{ device_id: {device} }}")
        );
    }
}
