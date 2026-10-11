//! What a person can do with tasks, lists and themselves. Each is a command on
//! the family; the aggregate checks it against the replayed state.

use anyhow::Result;
use tackly_protocol::{FamilyCommand, GeoPoint, Picture};
use uuid::Uuid;

use super::Device;

impl Device {
    // ---- tasks ---------------------------------------------------------------------------

    /// Adds a task to a list, or with `list_id: None` to "Other".
    pub async fn add_task(
        &mut self,
        title: &str,
        list_id: Option<Uuid>,
        place_ids: &[Uuid],
    ) -> Result<Uuid> {
        let task_id = Uuid::now_v7();
        self.run(FamilyCommand::AddTask {
            task_id,
            list_id,
            title: title.to_owned(),
            place_ids: place_ids.to_vec(),
        })
        .await?;
        Ok(task_id)
    }

    /// Takes the task back out (Undo of adding).
    pub async fn delete_task(&mut self, task_id: Uuid) -> Result<()> {
        self.run(FamilyCommand::DeleteTask { task_id }).await
    }

    /// Gives the task to someone (yourself included), or with `None` back to nobody.
    pub async fn assign_task(&mut self, task_id: Uuid, member_id: Option<Uuid>) -> Result<()> {
        self.run(FamilyCommand::AssignTask { task_id, member_id })
            .await
    }

    pub async fn start_task(&mut self, task_id: Uuid) -> Result<()> {
        self.run(FamilyCommand::StartTask { task_id }).await
    }

    pub async fn pause_task(&mut self, task_id: Uuid) -> Result<()> {
        self.run(FamilyCommand::PauseTask { task_id }).await
    }

    /// Finishes the task, noting where the phone is if it can tell.
    pub async fn complete_task(&mut self, task_id: Uuid, location: Option<GeoPoint>) -> Result<()> {
        self.run(FamilyCommand::CompleteTask { task_id, location })
            .await
    }

    /// Notes where the phone was when it finished the task (found out after).
    pub async fn locate_completion(&mut self, task_id: Uuid, location: GeoPoint) -> Result<()> {
        self.run(FamilyCommand::LocateCompletion { task_id, location })
            .await
    }

    /// Undo right after finishing: as it was, time included.
    pub async fn undo_completion(&mut self, task_id: Uuid) -> Result<()> {
        self.run(FamilyCommand::UndoCompletion { task_id }).await
    }

    /// Opens a finished task again; the time worked starts over.
    pub async fn reopen_task(&mut self, task_id: Uuid) -> Result<()> {
        self.run(FamilyCommand::ReopenTask { task_id }).await
    }

    // ---- lists ---------------------------------------------------------------------------

    pub async fn create_list(&mut self, name: &str) -> Result<Uuid> {
        let list_id = Uuid::now_v7();
        self.run(FamilyCommand::CreateList {
            list_id,
            name: name.to_owned(),
        })
        .await?;
        Ok(list_id)
    }

    pub async fn rename_list(&mut self, list_id: Uuid, name: &str) -> Result<()> {
        self.run(FamilyCommand::RenameList {
            list_id,
            name: name.to_owned(),
        })
        .await
    }

    /// Only a list without open tasks.
    pub async fn delete_list(&mut self, list_id: Uuid) -> Result<()> {
        self.run(FamilyCommand::DeleteList { list_id }).await
    }

    // ---- me --------------------------------------------------------------------------------

    pub async fn rename_me(&mut self, name: &str) -> Result<()> {
        self.run(FamilyCommand::Rename {
            name: name.to_owned(),
        })
        .await
    }

    pub async fn set_picture(&mut self, picture: Picture) -> Result<()> {
        self.run(FamilyCommand::SetPicture { picture }).await
    }
}
