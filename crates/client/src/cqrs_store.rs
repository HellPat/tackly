//! `cqrs-es` plumbing over the device's encrypted SQLite log. The family is
//! one aggregate whose stream is the log itself: server-ordered events first,
//! then this device's pending ones. Commits only append locally; the upload
//! trigger wakes the sync loop.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use cqrs_es::{Aggregate, AggregateError, EventEnvelope, EventStore as CqrsEventStore, Query};
use tackly_protocol::{Family, FamilyEvent};
use tokio::sync::Notify;

use crate::store::EventStore;

type StoreResult<T> = Result<T, AggregateError<<Family as Aggregate>::Error>>;

fn unexpected(
    error: impl Into<Box<dyn std::error::Error + Send + Sync>>,
) -> AggregateError<<Family as Aggregate>::Error> {
    AggregateError::UnexpectedError(error.into())
}

#[derive(Clone)]
pub struct DeviceStore {
    events: Arc<Mutex<EventStore>>,
}

impl DeviceStore {
    pub fn new(events: Arc<Mutex<EventStore>>) -> Self {
        Self { events }
    }

    fn envelopes(
        aggregate_id: &str,
        events: Vec<FamilyEvent>,
        first_sequence: usize,
    ) -> Vec<EventEnvelope<Family>> {
        events
            .into_iter()
            .enumerate()
            .map(|(index, payload)| EventEnvelope {
                aggregate_id: aggregate_id.to_owned(),
                sequence: first_sequence + index,
                metadata: HashMap::from([
                    ("event_id".to_owned(), payload.id.to_string()),
                    ("device_id".to_owned(), payload.origin_device_id.to_string()),
                ]),
                payload,
            })
            .collect()
    }
}

pub struct DeviceContext {
    aggregate: Family,
    sequence: usize,
}

impl cqrs_es::AggregateContext<Family> for DeviceContext {
    fn aggregate(&mut self) -> &mut Family {
        &mut self.aggregate
    }
}

impl CqrsEventStore<Family> for DeviceStore {
    type AC = DeviceContext;

    async fn load_events(&self, aggregate_id: &str) -> StoreResult<Vec<EventEnvelope<Family>>> {
        let events = self
            .events
            .lock()
            .unwrap()
            .read_events()
            .map_err(unexpected)?;
        Ok(Self::envelopes(aggregate_id, events, 1))
    }

    async fn load_aggregate(&self, aggregate_id: &str) -> StoreResult<DeviceContext> {
        let events = self.load_events(aggregate_id).await?;
        let mut aggregate = Family::default();
        for envelope in &events {
            aggregate.apply(envelope.payload.clone());
        }
        Ok(DeviceContext {
            aggregate,
            sequence: events.len(),
        })
    }

    async fn commit(
        &self,
        events: Vec<FamilyEvent>,
        context: DeviceContext,
        _metadata: HashMap<String, String>,
    ) -> StoreResult<Vec<EventEnvelope<Family>>> {
        {
            let store = self.events.lock().unwrap();
            for event in &events {
                store.insert(event, None).map_err(unexpected)?;
            }
        }
        let aggregate_id = context
            .aggregate
            .family_id
            .map(|id| id.to_string())
            .unwrap_or_default();
        Ok(Self::envelopes(&aggregate_id, events, context.sequence + 1))
    }
}

/// A `cqrs-es` query that only wakes the uploader after each commit.
pub struct UploadTrigger(pub Arc<Notify>);

#[async_trait::async_trait]
impl Query<Family> for UploadTrigger {
    async fn dispatch(&self, _aggregate_id: &str, _events: &[EventEnvelope<Family>]) {
        self.0.notify_one();
    }
}
