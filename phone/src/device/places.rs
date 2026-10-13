//! Places: groups such as "Grocery Store", the places in them such as "LIDL"
//! with their locations, and which tasks belong to which place.

use anyhow::Result;
use tackly_domain::{PlaceCommand, PlaceLocation, TaskCommand};
use uuid::Uuid;

use super::Device;

impl Device {
    pub async fn create_place_group(&mut self, name: &str) -> Result<Uuid> {
        let group_id = Uuid::now_v7();
        self.run(PlaceCommand::CreatePlaceGroup {
            group_id,
            name: name.parse()?,
        })
        .await?;
        Ok(group_id)
    }

    pub async fn rename_place_group(&mut self, group_id: Uuid, name: &str) -> Result<()> {
        self.run(PlaceCommand::RenamePlaceGroup {
            group_id,
            name: name.parse()?,
        })
        .await
    }

    /// Only a group without places.
    pub async fn delete_place_group(&mut self, group_id: Uuid) -> Result<()> {
        self.run(PlaceCommand::DeletePlaceGroup { group_id }).await
    }

    pub async fn create_place(
        &mut self,
        group_id: Uuid,
        name: &str,
        location: PlaceLocation,
    ) -> Result<Uuid> {
        let place_id = Uuid::now_v7();
        self.run(PlaceCommand::CreatePlace {
            place_id,
            group_id,
            name: name.parse()?,
            location,
        })
        .await?;
        Ok(place_id)
    }

    pub async fn rename_place(&mut self, place_id: Uuid, name: &str) -> Result<()> {
        self.run(PlaceCommand::RenamePlace {
            place_id,
            name: name.parse()?,
        })
        .await
    }

    pub async fn move_place(&mut self, place_id: Uuid, group_id: Uuid) -> Result<()> {
        self.run(PlaceCommand::MovePlace { place_id, group_id })
            .await
    }

    pub async fn delete_place(&mut self, place_id: Uuid) -> Result<()> {
        self.run(PlaceCommand::DeletePlace { place_id }).await
    }

    pub async fn add_place_location(
        &mut self,
        place_id: Uuid,
        location: PlaceLocation,
    ) -> Result<()> {
        self.run(PlaceCommand::AddPlaceLocation { place_id, location })
            .await
    }

    /// Not the last one: a place always keeps a location.
    pub async fn remove_place_location(&mut self, place_id: Uuid, location_id: Uuid) -> Result<()> {
        self.run(PlaceCommand::RemovePlaceLocation {
            place_id,
            location_id,
        })
        .await
    }

    pub async fn add_task_to_place(&mut self, task_id: Uuid, place_id: Uuid) -> Result<()> {
        self.run(TaskCommand::AddTaskToPlace { task_id, place_id })
            .await
    }

    pub async fn remove_task_from_place(&mut self, task_id: Uuid, place_id: Uuid) -> Result<()> {
        self.run(TaskCommand::RemoveTaskFromPlace { task_id, place_id })
            .await
    }
}
