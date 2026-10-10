//! The Places tab: groups such as "Grocery Store", the places in them, and
//! what there is to get at each. Opening a place shows its tasks.

use dioxus::prelude::*;
use tackly_protocol::{Family, Place};
use uuid::Uuid;

use super::tasks::TaskCard;
use crate::state::AppState;

#[component]
pub fn PlacesTab(
    open_place: EventHandler<Uuid>,
    new_group: EventHandler<()>,
    new_place: EventHandler<Uuid>,
) -> Element {
    let state = use_context::<AppState>();
    let family = (state.family)();
    rsx! {
        if family.place_groups.is_empty() {
            div { class: "empty",
                div { class: "big", "📍" }
                p { "No places yet. Make a group such as Grocery Store, then add the shops in it." }
            }
        }
        for group in family.place_groups.values() {
            div { key: "{group.id}", class: "group-head",
                span { class: "section", "{group.emoji} {group.name}" }
                button { class: "btn text", onclick: {
                        let id = group.id;
                        move |_| new_place.call(id)
                    },
                    "Add place"
                }
            }
            for place in places_by_open_tasks(&family, group.id) {
                PlaceCard { key: "{place.id}", place: place.clone(), open_place }
            }
        }
        div { class: "actions",
            button { class: "btn tonal", onclick: move |_| new_group.call(()), "New group" }
        }
    }
}

/// The place with the most to get comes first, to help choose where to go.
fn places_by_open_tasks(family: &Family, group: Uuid) -> Vec<Place> {
    let mut places: Vec<Place> = family
        .places
        .values()
        .filter(|place| place.group_id == group)
        .cloned()
        .collect();
    places.sort_by_key(|place| std::cmp::Reverse(family.open_tasks_at(place.id).count()));
    places
}

#[component]
fn PlaceCard(place: Place, open_place: EventHandler<Uuid>) -> Element {
    let state = use_context::<AppState>();
    let count = (state.family)().open_tasks_at(place.id).count();
    let id = place.id;
    let first = place.locations.first().map(|location| {
        location
            .address
            .clone()
            .unwrap_or_else(|| location.name.clone())
    });
    let more = place.locations.len().saturating_sub(1);
    rsx! {
        button { class: "card place-card", onclick: move |_| open_place.call(id),
            div { class: "emoji", aria_hidden: "true", "{place.emoji}" }
            div { class: "body",
                div { class: "title", "{place.name}" }
                if let Some(first) = first {
                    div { class: "meta",
                        "{first}"
                        if more > 0 {
                            " · +{more} more"
                        }
                    }
                }
            }
            span { class: "count", aria_label: "{count} to get", "{count}" }
        }
    }
}

/// One place: where it is, and what there is to get there.
#[component]
pub fn PlaceView(
    place: Uuid,
    back: EventHandler<()>,
    add_location: EventHandler<Uuid>,
    open_finish: EventHandler<Uuid>,
    edit_places: EventHandler<Uuid>,
) -> Element {
    let state = use_context::<AppState>();
    let family = (state.family)();
    let Some(current) = family.places.get(&place) else {
        return rsx! {};
    };
    let tasks: Vec<_> = family.open_tasks_at(place).cloned().collect();
    rsx! {
        div { class: "place-head",
            button { class: "btn text", aria_label: "Back", onclick: move |_| back.call(()), "←" }
            h2 { "{current.emoji} {current.name}" }
        }
        for location in &current.locations {
            div { key: "{location.id}", class: "meta",
                "📍 {location.name}"
                if let Some(address) = &location.address {
                    " · {address}"
                }
            }
        }
        div { class: "actions",
            button { class: "btn text", onclick: move |_| add_location.call(place), "Add location" }
        }
        if tasks.is_empty() {
            div { class: "empty", p { "Nothing to get here." } }
        }
        for task in tasks {
            TaskCard { key: "{task.id}", task, open_finish, edit_places }
        }
    }
}
