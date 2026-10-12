//! Places, in groups such as "Grocery Store", with their locations.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{
    commands::FamilyCommand,
    errors::FamilyError,
    events::{DomainEvent, FamilyEvent, PlaceLocation},
    state::Family,
    tasks::Task,
};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlaceGroup {
    pub id: Uuid,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Place {
    pub id: Uuid,
    pub group_id: Uuid,
    pub name: String,
    /// Never empty.
    pub locations: Vec<PlaceLocation>,
}

impl Family {
    /// Open tasks to do at a place.
    pub fn open_tasks_at(&self, place_id: Uuid) -> impl Iterator<Item = &Task> {
        self.open_tasks()
            .filter(move |task| task.place_ids.contains(&place_id))
    }

    pub fn places_in(&self, group_id: Uuid) -> impl Iterator<Item = &Place> {
        self.places
            .values()
            .filter(move |place| place.group_id == group_id)
    }

    /// Checks the places commands and returns the events they produce, as (subject, event).
    pub(super) fn decide_places(
        &self,
        command: FamilyCommand,
        _me: Uuid,
    ) -> Result<Vec<(Uuid, DomainEvent)>, FamilyError> {
        use DomainEvent as E;
        use FamilyCommand as C;
        let group = |id: Uuid| self.place_groups.get(&id).ok_or(FamilyError::UnknownGroup);
        let place = |id: Uuid| self.places.get(&id).ok_or(FamilyError::UnknownPlace);
        let one = |subject, event| Ok(vec![(subject, event)]);
        match command {
            C::CreatePlaceGroup { group_id, name } => one(group_id, E::PlaceGroupCreated { name }),
            C::RenamePlaceGroup { group_id, name } => {
                group(group_id)?;
                one(group_id, E::PlaceGroupRenamed { name })
            }
            C::DeletePlaceGroup { group_id } => {
                group(group_id)?;
                if self.places_in(group_id).next().is_some() {
                    return Err(FamilyError::GroupNotEmpty);
                }
                one(group_id, E::PlaceGroupDeleted)
            }
            C::CreatePlace {
                place_id,
                group_id,
                name,
                location,
            } => {
                group(group_id)?;
                one(
                    place_id,
                    E::PlaceCreated {
                        group_id,
                        name,
                        location,
                    },
                )
            }
            C::RenamePlace { place_id, name } => {
                place(place_id)?;
                one(place_id, E::PlaceRenamed { name })
            }
            C::MovePlace { place_id, group_id } => {
                place(place_id)?;
                group(group_id)?;
                one(place_id, E::PlaceMoved { group_id })
            }
            C::DeletePlace { place_id } => {
                place(place_id)?;
                one(place_id, E::PlaceDeleted)
            }
            C::AddPlaceLocation { place_id, location } => {
                place(place_id)?;
                one(place_id, E::PlaceLocationAdded { location })
            }
            C::RemovePlaceLocation {
                place_id,
                location_id,
            } => {
                if place(place_id)?.locations.len() <= 1 {
                    return Err(FamilyError::LastLocation);
                }
                one(place_id, E::PlaceLocationRemoved { location_id })
            }
            _ => Ok(Vec::new()),
        }
    }

    /// Applies the places events; `false` when the event is not one of them.
    pub(super) fn apply_places(&mut self, envelope: &FamilyEvent) -> bool {
        let (subject, _, _) = (
            envelope.subject_id,
            envelope.origin_device_id,
            envelope.occurred_at,
        );
        match &envelope.event {
            DomainEvent::PlaceGroupCreated { name } => {
                self.place_groups.entry(subject).or_insert(PlaceGroup {
                    id: subject,
                    name: name.to_string(),
                });
            }
            DomainEvent::PlaceGroupRenamed { name } => {
                if let Some(group) = self.place_groups.get_mut(&subject) {
                    group.name = name.to_string();
                }
            }
            DomainEvent::PlaceGroupDeleted => {
                if self.places_in(subject).next().is_none() {
                    self.place_groups.remove(&subject);
                }
            }
            DomainEvent::PlaceCreated {
                group_id,
                name,
                location,
            } => {
                if self.place_groups.contains_key(group_id) {
                    self.places.entry(subject).or_insert(Place {
                        id: subject,
                        group_id: *group_id,
                        name: name.to_string(),
                        locations: vec![location.clone()],
                    });
                }
            }
            DomainEvent::PlaceRenamed { name } => {
                if let Some(place) = self.places.get_mut(&subject) {
                    place.name = name.to_string();
                }
            }
            DomainEvent::PlaceMoved { group_id } => {
                if self.place_groups.contains_key(group_id)
                    && let Some(place) = self.places.get_mut(&subject)
                {
                    place.group_id = *group_id;
                }
            }
            DomainEvent::PlaceDeleted => {
                if self.places.remove(&subject).is_some() {
                    for task in self.tasks.values_mut() {
                        task.place_ids.retain(|id| *id != subject);
                    }
                }
            }
            DomainEvent::PlaceLocationAdded { location } => {
                if let Some(place) = self.places.get_mut(&subject)
                    && !place.locations.iter().any(|known| known.id == location.id)
                {
                    place.locations.push(location.clone());
                }
            }
            DomainEvent::PlaceLocationRemoved { location_id } => {
                if let Some(place) = self.places.get_mut(&subject)
                    && place.locations.len() > 1
                {
                    place
                        .locations
                        .retain(|location| location.id != *location_id);
                }
            }
            _ => return false,
        }
        true
    }
}
