//! The family aggregate: commands are checked against the replayed state and
//! produce events. `Family::apply` stays lenient, because events from other
//! devices arrive in server order and may overlap (see `apply_event`).

use chrono::Utc;
use cqrs_es::{Aggregate, event_sink::EventSink};
use uuid::Uuid;

use crate::{
    CompletionMetadata, DomainEvent, Family, FamilyEvent, GeoPoint, PlaceLocation, TaskStatus,
};

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
        place_ids: Vec<Uuid>,
    },
    CreatePlaceGroup {
        group_id: Uuid,
        name: String,
        emoji: String,
    },
    CreatePlace {
        place_id: Uuid,
        group_id: Uuid,
        name: String,
        emoji: String,
        location: PlaceLocation,
    },
    AddPlaceLocation {
        place_id: Uuid,
        location: PlaceLocation,
    },
    AddTaskToPlace {
        task_id: Uuid,
        place_id: Uuid,
    },
    RemoveTaskFromPlace {
        task_id: Uuid,
        place_id: Uuid,
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
    #[error("unknown place group")]
    UnknownGroup,
    #[error("unknown place")]
    UnknownPlace,
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

fn clean_location(mut location: PlaceLocation) -> Result<PlaceLocation, FamilyError> {
    location.name = location.name.trim().to_owned();
    if location.name.is_empty() {
        return Err(FamilyError::Empty);
    }
    Ok(location)
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
            // Random, not time-ordered: the server sees event IDs.
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
                place_ids,
            } => {
                if !self.lists.contains_key(&list_id) {
                    return Err(FamilyError::UnknownList);
                }
                if place_ids.iter().any(|id| !self.places.contains_key(id)) {
                    return Err(FamilyError::UnknownPlace);
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
                            place_ids,
                        },
                    ),
                    self,
                )
                .await;
            }
            FamilyCommand::CreatePlaceGroup {
                group_id,
                name,
                emoji,
            } => {
                let name = not_empty(&name)?;
                sink.write(
                    event(group_id, DomainEvent::PlaceGroupCreated { name, emoji }),
                    self,
                )
                .await;
            }
            FamilyCommand::CreatePlace {
                place_id,
                group_id,
                name,
                emoji,
                location,
            } => {
                if !self.place_groups.contains_key(&group_id) {
                    return Err(FamilyError::UnknownGroup);
                }
                let name = not_empty(&name)?;
                let location = clean_location(location)?;
                sink.write(
                    event(
                        place_id,
                        DomainEvent::PlaceCreated {
                            group_id,
                            name,
                            emoji,
                            location,
                        },
                    ),
                    self,
                )
                .await;
            }
            FamilyCommand::AddPlaceLocation { place_id, location } => {
                if !self.places.contains_key(&place_id) {
                    return Err(FamilyError::UnknownPlace);
                }
                let location = clean_location(location)?;
                sink.write(
                    event(place_id, DomainEvent::PlaceLocationAdded { location }),
                    self,
                )
                .await;
            }
            FamilyCommand::AddTaskToPlace { task_id, place_id } => {
                let task = self.tasks.get(&task_id).ok_or(FamilyError::UnknownTask)?;
                if !self.places.contains_key(&place_id) {
                    return Err(FamilyError::UnknownPlace);
                }
                if !task.place_ids.contains(&place_id) {
                    sink.write(
                        event(task_id, DomainEvent::TaskPlaceAdded { place_id }),
                        self,
                    )
                    .await;
                }
            }
            FamilyCommand::RemoveTaskFromPlace { task_id, place_id } => {
                let task = self.tasks.get(&task_id).ok_or(FamilyError::UnknownTask)?;
                if task.place_ids.contains(&place_id) {
                    sink.write(
                        event(task_id, DomainEvent::TaskPlaceRemoved { place_id }),
                        self,
                    )
                    .await;
                }
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
            device_id: Uuid::now_v7(),
        };
        let cqrs = CqrsFramework::new(MemStore::<Family>::default(), vec![], services);
        let (family, list, task) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
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
                place_ids: vec![],
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

    #[tokio::test]
    async fn tasks_are_assigned_to_places_that_exist() {
        let services = Services {
            device_id: Uuid::now_v7(),
        };
        let cqrs = CqrsFramework::new(MemStore::<Family>::default(), vec![], services);
        let (family, list, group, place, task) = (
            Uuid::now_v7(),
            Uuid::now_v7(),
            Uuid::now_v7(),
            Uuid::now_v7(),
            Uuid::now_v7(),
        );
        let id = family.to_string();
        let run = |command| cqrs.execute(&id, command);
        run(FamilyCommand::CreateFamily {
            family_id: family,
            list_id: list,
            name: "Home".into(),
            owner_name: "A".into(),
        })
        .await
        .unwrap();
        let add = |place_ids| FamilyCommand::AddTask {
            task_id: task,
            list_id: list,
            title: "Milk".into(),
            emoji: "🥛".into(),
            place_ids,
        };
        assert!(matches!(
            run(add(vec![place])).await,
            Err(cqrs_es::AggregateError::UserError(
                FamilyError::UnknownPlace
            ))
        ));
        run(FamilyCommand::CreatePlaceGroup {
            group_id: group,
            name: "Grocery Store".into(),
            emoji: "🛒".into(),
        })
        .await
        .unwrap();
        run(FamilyCommand::CreatePlace {
            place_id: place,
            group_id: group,
            name: "LIDL".into(),
            emoji: "🏪".into(),
            location: PlaceLocation::named("LIDL Winnenden"),
        })
        .await
        .unwrap();
        run(add(vec![place])).await.unwrap();
    }
}
