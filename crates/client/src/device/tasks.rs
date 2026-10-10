//! What a person can do with tasks. Each is a command on the family; the
//! aggregate checks it against the replayed state.

use anyhow::{Context, Result};
use tackly_protocol::{FamilyCommand, GeoPoint};
use uuid::Uuid;

use super::Device;

impl Device {
    fn default_list(&self) -> Result<Uuid> {
        self.state()?
            .lists
            .keys()
            .next()
            .copied()
            .context("the family has no list yet")
    }

    pub async fn add_task(&mut self, title: &str, emoji: &str, place_ids: &[Uuid]) -> Result<Uuid> {
        let task_id = Uuid::now_v7();
        self.run(FamilyCommand::AddTask {
            task_id,
            list_id: self.default_list()?,
            title: title.to_owned(),
            emoji: emoji.to_owned(),
            place_ids: place_ids.to_vec(),
        })
        .await?;
        Ok(task_id)
    }

    pub async fn start_task(&mut self, task_id: Uuid) -> Result<()> {
        self.run(FamilyCommand::StartTask { task_id }).await
    }

    /// The duration and the start it refers to come from the replayed
    /// `task.started`; the caller supplies the note and the location.
    pub async fn complete_task(
        &mut self,
        task_id: Uuid,
        note: Option<String>,
        location: Option<GeoPoint>,
    ) -> Result<()> {
        self.run(FamilyCommand::CompleteTask {
            task_id,
            note,
            location,
        })
        .await
    }

    pub async fn reopen_task(&mut self, task_id: Uuid) -> Result<()> {
        self.run(FamilyCommand::ReopenTask { task_id }).await
    }

    /// Any member who finished the task may settle a double completion.
    pub async fn resolve_conflict(&mut self, task_id: Uuid, keep: Uuid) -> Result<()> {
        self.run(FamilyCommand::ResolveConflict {
            task_id,
            keep_completion_event_id: keep,
        })
        .await
    }
}
