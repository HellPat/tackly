//! The family aggregate: commands are checked against the replayed state and
//! produce events. `Family::apply` stays lenient, because events from other
//! devices arrive in server order and may overlap (see `apply_event`).

use chrono::Utc;
use cqrs_es::{Aggregate, event_sink::EventSink};
use uuid::Uuid;

use crate::{CompletionMetadata, DomainEvent, Family, FamilyEvent, GeoPoint, TaskStatus};

#[derive(Clone, Debug)]
pub enum FamilyCommand {
    CreateFamily {
        family_id: Uuid,
        list_id: Uuid,
        name: String,
        owner_name: String,
    },
    Join {
        name: String,
    },
    AddTask {
        task_id: Uuid,
        list_id: Uuid,
        title: String,
        emoji: String,
    },
    StartTask {
        task_id: Uuid,
    },
    CompleteTask {
        task_id: Uuid,
        note: Option<String>,
        location: Option<GeoPoint>,
    },
    ReopenTask {
        task_id: Uuid,
    },
    /// Anyone who finished the task may settle a double completion.
    ResolveConflict {
        task_id: Uuid,
        keep_completion_event_id: Uuid,
    },
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum FamilyError {
    #[error("this phone is already in a family")]
    AlreadyCreated,
    #[error("names and titles must not be empty")]
    Empty,
    #[error("unknown list")]
    UnknownList,
    #[error("unknown task")]
    UnknownTask,
    #[error("the task is already done")]
    AlreadyDone,
    #[error("the task is not done")]
    NotDone,
    #[error("the task has no conflict")]
    NoConflict,
    #[error("only members who finished this task can resolve it")]
    NotParticipant,
    #[error("that completion is not part of the conflict")]
    UnknownClaim,
}

/// Who is acting. Time comes from the system clock.
#[derive(Clone, Debug)]
pub struct Services {
    pub device_id: Uuid,
}

impl Aggregate for Family {
    const TYPE: &'static str = "family";
    type Command = FamilyCommand;
    type Event = FamilyEvent;
    type Error = FamilyError;
    type Services = Services;

    async fn handle(
        &mut self,
        command: FamilyCommand,
        services: &Services,
        sink: &EventSink<Self>,
    ) -> Result<(), FamilyError> {
        let event = |subject_id, event| FamilyEvent {
            id: Uuid::new_v4(),
            subject_id,
            origin_device_id: services.device_id,
            occurred_at: Utc::now(),
            event,
        };
        let not_empty = |text: &str| -> Result<String, FamilyError> {
            let text = text.trim();
            if text.is_empty() {
                Err(FamilyError::Empty)
            } else {
                Ok(text.to_owned())
            }
        };
        match command {
            FamilyCommand::CreateFamily {
                family_id,
                list_id,
                name,
                owner_name,
            } => {
                if self.family_id.is_some() {
                    return Err(FamilyError::AlreadyCreated);
                }
                let (name, owner_name) = (not_empty(&name)?, not_empty(&owner_name)?);
                sink.write(
                    event(family_id, DomainEvent::FamilyCreated { name, owner_name }),
                    self,
                )
                .await;
                sink.write(
                    event(
                        list_id,
                        DomainEvent::ListCreated {
                            name: "Household".into(),
                            emoji: "🏠".into(),
                        },
                    ),
                    self,
                )
                .await;
            }
            FamilyCommand::Join { name } => {
                let family_id = self.family_id.ok_or(FamilyError::UnknownList)?;
                let name = not_empty(&name)?;
                sink.write(event(family_id, DomainEvent::MemberJoined { name }), self)
                    .await;
            }
            FamilyCommand::AddTask {
                task_id,
                list_id,
                title,
                emoji,
            } => {
                if !self.lists.contains_key(&list_id) {
                    return Err(FamilyError::UnknownList);
                }
                let title = not_empty(&title)?;
                let emoji = if emoji.is_empty() {
                    "✅".into()
                } else {
                    emoji
                };
                sink.write(
                    event(
                        task_id,
                        DomainEvent::TaskCreated {
                            list_id,
                            title,
                            emoji,
                        },
                    ),
                    self,
                )
                .await;
            }
            FamilyCommand::StartTask { task_id } => {
                let task = self.tasks.get(&task_id).ok_or(FamilyError::UnknownTask)?;
                if task.is_done() {
                    return Err(FamilyError::AlreadyDone);
                }
                sink.write(event(task_id, DomainEvent::TaskStarted), self)
                    .await;
            }
            FamilyCommand::CompleteTask {
                task_id,
                note,
                location,
            } => {
                let task = self.tasks.get(&task_id).ok_or(FamilyError::UnknownTask)?;
                let (started_event_id, duration_seconds) = match &task.status {
                    TaskStatus::Done { .. } => return Err(FamilyError::AlreadyDone),
                    TaskStatus::InProgress {
                        since,
                        start_event_id,
                        ..
                    } => (
                        Some(*start_event_id),
                        Some((Utc::now() - *since).num_seconds().max(0)),
                    ),
                    TaskStatus::Open => (None, None),
                };
                let metadata = CompletionMetadata {
                    started_event_id,
                    duration_seconds,
                    note: note.filter(|note| !note.trim().is_empty()),
                    location,
                };
                sink.write(event(task_id, DomainEvent::TaskCompleted(metadata)), self)
                    .await;
            }
            FamilyCommand::ReopenTask { task_id } => {
                let task = self.tasks.get(&task_id).ok_or(FamilyError::UnknownTask)?;
                let TaskStatus::Done {
                    completion_event_id,
                    ..
                } = &task.status
                else {
                    return Err(FamilyError::NotDone);
                };
                let completion_event_id = *completion_event_id;
                sink.write(
                    event(
                        task_id,
                        DomainEvent::TaskReopened {
                            completion_event_id,
                        },
                    ),
                    self,
                )
                .await;
            }
            FamilyCommand::ResolveConflict {
                task_id,
                keep_completion_event_id,
            } => {
                let task = self.tasks.get(&task_id).ok_or(FamilyError::UnknownTask)?;
                if !task.has_conflict() {
                    return Err(FamilyError::NoConflict);
                }
                if !task
                    .claims
                    .iter()
                    .any(|claim| claim.by == services.device_id)
                {
                    return Err(FamilyError::NotParticipant);
                }
                if !task
                    .claims
                    .iter()
                    .any(|claim| claim.completion_event_id == keep_completion_event_id)
                {
                    return Err(FamilyError::UnknownClaim);
                }
                let resolved_event_ids = task
                    .claims
                    .iter()
                    .map(|claim| claim.completion_event_id)
                    .collect();
                sink.write(
                    event(
                        task_id,
                        DomainEvent::TaskConflictResolved {
                            kept_completion_event_id: keep_completion_event_id,
                            resolved_event_ids,
                        },
                    ),
                    self,
                )
                .await;
            }
        }
        Ok(())
    }

    fn apply(&mut self, event: FamilyEvent) {
        self.apply_event(&event);
    }
}

#[cfg(test)]
mod tests {
    use cqrs_es::{CqrsFramework, mem_store::MemStore};

    use super::*;

    #[tokio::test]
    async fn commands_are_checked_and_events_replayed() {
        let services = Services {
            device_id: Uuid::new_v4(),
        };
        let cqrs = CqrsFramework::new(MemStore::<Family>::default(), vec![], services);
        let (family, list, task) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        let id = family.to_string();
        cqrs.execute(
            &id,
            FamilyCommand::CreateFamily {
                family_id: family,
                list_id: list,
                name: "Home".into(),
                owner_name: "A".into(),
            },
        )
        .await
        .unwrap();
        cqrs.execute(
            &id,
            FamilyCommand::AddTask {
                task_id: task,
                list_id: list,
                title: "Dishes".into(),
                emoji: "🍽".into(),
            },
        )
        .await
        .unwrap();
        cqrs.execute(&id, FamilyCommand::StartTask { task_id: task })
            .await
            .unwrap();
        cqrs.execute(
            &id,
            FamilyCommand::CompleteTask {
                task_id: task,
                note: Some("ok".into()),
                location: None,
            },
        )
        .await
        .unwrap();
        let again = cqrs
            .execute(
                &id,
                FamilyCommand::CompleteTask {
                    task_id: task,
                    note: None,
                    location: None,
                },
            )
            .await;
        assert!(matches!(
            again,
            Err(cqrs_es::AggregateError::UserError(FamilyError::AlreadyDone))
        ));
    }
}
