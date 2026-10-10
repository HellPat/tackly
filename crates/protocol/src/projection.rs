//! Replays events into the visible family state. Events are applied in server
//! order, followed by this device's not-yet-uploaded events.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::events::{CompletionMetadata, DomainEvent, FamilyEvent};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Member {
    pub device_id: Uuid,
    pub name: String,
    pub owner: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TaskList {
    pub id: Uuid,
    pub name: String,
    pub emoji: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum TaskStatus {
    Open,
    InProgress {
        by: Uuid,
        since: DateTime<Utc>,
        start_event_id: Uuid,
    },
    Done {
        by: Uuid,
        at: DateTime<Utc>,
        completion_event_id: Uuid,
        metadata: CompletionMetadata,
    },
}

/// One member's claim to have finished a task.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CompletionClaim {
    pub by: Uuid,
    pub at: DateTime<Utc>,
    pub completion_event_id: Uuid,
    pub metadata: CompletionMetadata,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Task {
    pub id: Uuid,
    pub list_id: Uuid,
    pub title: String,
    pub emoji: String,
    pub created_by: Uuid,
    pub status: TaskStatus,
    /// Competing completions, first-in-server-order first. Empty unless two
    /// members both finished the task. Any member who wrote one of them may
    /// settle it with `task.conflict_resolved`.
    pub claims: Vec<CompletionClaim>,
}

impl Task {
    pub fn has_conflict(&self) -> bool {
        self.claims.len() > 1
    }

    pub fn is_done(&self) -> bool {
        matches!(self.status, TaskStatus::Done { .. })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActivityKind {
    FamilyCreated,
    Joined,
    Created,
    Started,
    Finished,
    Reopened,
    Conflicted,
    Resolved,
}

/// A human-readable history line, newest last.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Activity {
    pub event_id: Uuid,
    pub at: DateTime<Utc>,
    pub by: Uuid,
    pub kind: ActivityKind,
    pub subject: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Family {
    pub family_id: Option<Uuid>,
    pub name: Option<String>,
    pub members: BTreeMap<Uuid, Member>,
    pub lists: BTreeMap<Uuid, TaskList>,
    pub tasks: BTreeMap<Uuid, Task>,
    pub activity: Vec<Activity>,
}

impl Family {
    /// Rebuilds the family by replaying events in order.
    pub fn replay<'a>(events: impl IntoIterator<Item = &'a FamilyEvent>) -> Self {
        let mut state = Self::default();
        for event in events {
            state.apply_event(event);
        }
        state
    }

    pub fn member_name(&self, device_id: Uuid) -> &str {
        self.members
            .get(&device_id)
            .map_or("Someone", |member| member.name.as_str())
    }

    pub fn tasks_in(&self, list_id: Uuid) -> impl Iterator<Item = &Task> {
        self.tasks
            .values()
            .filter(move |task| task.list_id == list_id)
    }

    pub fn task_by_title(&self, title: &str) -> Option<&Task> {
        self.tasks.values().find(|task| task.title == title)
    }

    /// Applies one event. Events that do not fit the current state (a second
    /// completion, a start after completion, a stale reopen) are ignored, so
    /// replay never fails and every device converges on the same result.
    pub fn apply_event(&mut self, envelope: &FamilyEvent) {
        let by = envelope.origin_device_id;
        let at = envelope.occurred_at;
        let activity = |state: &mut Self, kind, subject: String| {
            state.activity.push(Activity {
                event_id: envelope.id,
                at,
                by,
                kind,
                subject,
            })
        };
        match &envelope.event {
            DomainEvent::FamilyCreated { name, owner_name } => {
                if self.family_id.is_some() {
                    return;
                }
                self.family_id = Some(envelope.subject_id);
                self.name = Some(name.clone());
                self.members.insert(
                    by,
                    Member {
                        device_id: by,
                        name: owner_name.clone(),
                        owner: true,
                    },
                );
                activity(self, ActivityKind::FamilyCreated, name.clone());
            }
            DomainEvent::MemberJoined { name } => {
                let owner = self.members.get(&by).is_some_and(|member| member.owner);
                self.members.insert(
                    by,
                    Member {
                        device_id: by,
                        name: name.clone(),
                        owner,
                    },
                );
                activity(self, ActivityKind::Joined, name.clone());
            }
            DomainEvent::ListCreated { name, emoji } => {
                self.lists.entry(envelope.subject_id).or_insert(TaskList {
                    id: envelope.subject_id,
                    name: name.clone(),
                    emoji: emoji.clone(),
                });
            }
            DomainEvent::TaskCreated {
                list_id,
                title,
                emoji,
            } => {
                if self.tasks.contains_key(&envelope.subject_id) {
                    return;
                }
                self.tasks.insert(
                    envelope.subject_id,
                    Task {
                        id: envelope.subject_id,
                        list_id: *list_id,
                        title: title.clone(),
                        emoji: emoji.clone(),
                        created_by: by,
                        status: TaskStatus::Open,
                        claims: Vec::new(),
                    },
                );
                activity(self, ActivityKind::Created, title.clone());
            }
            DomainEvent::TaskStarted => {
                let Some(task) = self.tasks.get_mut(&envelope.subject_id) else {
                    return;
                };
                if task.is_done() {
                    return;
                }
                task.status = TaskStatus::InProgress {
                    by,
                    since: at,
                    start_event_id: envelope.id,
                };
                let title = task.title.clone();
                activity(self, ActivityKind::Started, title);
            }
            DomainEvent::TaskCompleted(metadata) => {
                let Some(task) = self.tasks.get_mut(&envelope.subject_id) else {
                    return;
                };
                task.claims.push(CompletionClaim {
                    by,
                    at,
                    completion_event_id: envelope.id,
                    metadata: metadata.clone(),
                });
                if task.is_done() {
                    // A second finisher: keep the first, flag the conflict.
                    let title = task.title.clone();
                    activity(self, ActivityKind::Conflicted, title);
                    return;
                }
                task.status = TaskStatus::Done {
                    by,
                    at,
                    completion_event_id: envelope.id,
                    metadata: metadata.clone(),
                };
                let title = task.title.clone();
                activity(self, ActivityKind::Finished, title);
            }
            DomainEvent::TaskReopened {
                completion_event_id,
            } => {
                let Some(task) = self.tasks.get_mut(&envelope.subject_id) else {
                    return;
                };
                if !matches!(&task.status, TaskStatus::Done { completion_event_id: done, .. } if done == completion_event_id)
                {
                    return;
                }
                task.status = TaskStatus::Open;
                task.claims.clear();
                let title = task.title.clone();
                activity(self, ActivityKind::Reopened, title);
            }
            DomainEvent::TaskConflictResolved {
                kept_completion_event_id,
                resolved_event_ids,
            } => {
                let Some(task) = self.tasks.get_mut(&envelope.subject_id) else {
                    return;
                };
                // Anyone who took part in the conflict may resolve it.
                let participant = task.claims.iter().any(|claim| claim.by == by);
                let covers_all = task
                    .claims
                    .iter()
                    .all(|claim| resolved_event_ids.contains(&claim.completion_event_id));
                let Some(kept) = task
                    .claims
                    .iter()
                    .find(|claim| claim.completion_event_id == *kept_completion_event_id)
                    .cloned()
                else {
                    return;
                };
                if !task.has_conflict() || !participant || !covers_all {
                    return;
                }
                task.status = TaskStatus::Done {
                    by: kept.by,
                    at: kept.at,
                    completion_event_id: kept.completion_event_id,
                    metadata: kept.metadata.clone(),
                };
                task.claims = vec![kept];
                let title = task.title.clone();
                activity(self, ActivityKind::Resolved, title);
            }
            DomainEvent::Unknown => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(subject_id: Uuid, by: Uuid, event: DomainEvent) -> FamilyEvent {
        FamilyEvent {
            id: Uuid::new_v4(),
            subject_id,
            origin_device_id: by,
            occurred_at: Utc::now(),
            event,
        }
    }

    #[test]
    fn first_completion_wins_and_reopen_needs_the_current_completion() {
        let (family, list, task) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        let (owner, member) = (Uuid::new_v4(), Uuid::new_v4());
        let first = event(task, member, DomainEvent::TaskCompleted(Default::default()));
        let events = vec![
            event(
                family,
                owner,
                DomainEvent::FamilyCreated {
                    name: "Home".into(),
                    owner_name: "A".into(),
                },
            ),
            event(
                member,
                member,
                DomainEvent::MemberJoined { name: "B".into() },
            ),
            event(
                list,
                owner,
                DomainEvent::ListCreated {
                    name: "Chores".into(),
                    emoji: "🧹".into(),
                },
            ),
            event(
                task,
                owner,
                DomainEvent::TaskCreated {
                    list_id: list,
                    title: "Dishes".into(),
                    emoji: "🍽".into(),
                },
            ),
            event(task, owner, DomainEvent::TaskStarted),
            first.clone(),
            event(task, owner, DomainEvent::TaskCompleted(Default::default())),
            event(
                task,
                owner,
                DomainEvent::TaskReopened {
                    completion_event_id: Uuid::new_v4(),
                },
            ),
        ];
        let state = Family::replay(&events);
        assert_eq!(state.member_name(member), "B");
        match &state.tasks[&task].status {
            TaskStatus::Done {
                by,
                completion_event_id,
                ..
            } => {
                assert_eq!(*by, member);
                assert_eq!(*completion_event_id, first.id);
            }
            other => panic!("unexpected {other:?}"),
        }
        let reopened = event(
            task,
            owner,
            DomainEvent::TaskReopened {
                completion_event_id: first.id,
            },
        );
        let mut state = state;
        state.apply_event(&reopened);
        assert_eq!(state.tasks[&task].status, TaskStatus::Open);
    }

    #[test]
    fn only_participants_resolve_a_conflict() {
        let (list, task) = (Uuid::new_v4(), Uuid::new_v4());
        let (a, b, c) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        let done_a = event(task, a, DomainEvent::TaskCompleted(Default::default()));
        let done_b = event(task, b, DomainEvent::TaskCompleted(Default::default()));
        let mut state = Family::replay(&[
            event(
                task,
                a,
                DomainEvent::TaskCreated {
                    list_id: list,
                    title: "Trash".into(),
                    emoji: "🗑".into(),
                },
            ),
            done_a.clone(),
            done_b.clone(),
        ]);
        assert!(state.tasks[&task].has_conflict());
        let resolve = |by| {
            event(
                task,
                by,
                DomainEvent::TaskConflictResolved {
                    kept_completion_event_id: done_b.id,
                    resolved_event_ids: vec![done_a.id, done_b.id],
                },
            )
        };
        state.apply_event(&resolve(c));
        assert!(state.tasks[&task].has_conflict(), "outsider is ignored");
        state.apply_event(&resolve(b));
        assert!(!state.tasks[&task].has_conflict());
        assert!(matches!(
            &state.tasks[&task].status,
            TaskStatus::Done { by, .. } if *by == b
        ));
    }
}
