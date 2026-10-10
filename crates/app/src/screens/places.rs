//! The Places tab: groups ("Grocery Store") to step into, their places
//! ("LIDL") sorted by what there is to get, a place's tasks, and Edit place.

use dioxus::prelude::*;
use tackly_protocol::Place;

use super::{
    home::{Nav, SheetKind},
    tasks::TaskList,
};
use crate::{
    state::AppState,
    ui::{CARD, CHIP_OFF, CHIP_ON, EmptyState, FIELD, Heading, Icon, OverviewRow},
};

#[component]
pub fn PlacesTab() -> Element {
    let state = use_context::<AppState>();
    let mut nav = use_context::<Signal<Nav>>();
    let family = (state.family)();
    let here = nav();

    if let Some(place) = here.place.and_then(|id| family.places.get(&id).cloned()) {
        if here.editing_place {
            return rsx! { EditPlace { place } };
        }
        let n = place.locations.len();
        let tasks = family.open_tasks_at(place.id).cloned().collect();
        return rsx! {
            button {
                class: "flex items-center gap-1.5 px-2 pb-3 text-sm text-stone-700 hover:text-stone-900",
                onclick: move |_| nav.write().editing_place = true,
                Icon { name: "location_on", class: "!text-[18px] text-stone-600" }
                if n == 1 { "1 location" } else { "{n} locations" }
            }
            TaskList { tasks, empty_line: "Nothing to get here." }
        };
    }

    if let Some(group) = here.group {
        let mut places: Vec<&Place> = family.places_in(group).collect();
        places.sort_by_key(|place| std::cmp::Reverse(family.open_tasks_at(place.id).count()));
        if places.is_empty() {
            return rsx! { EmptyState { icon: "storefront", title: "No places here yet", line: "Add one below, for example LIDL." } };
        }
        return rsx! {
            ul { class: "{CARD} overflow-hidden mt-1",
                for place in places {
                    OverviewRow {
                        key: "{place.id}",
                        icon: "storefront",
                        name: place.name.clone(),
                        count: match family.open_tasks_at(place.id).count() {
                            0 => String::new(),
                            n => format!("{n} to get"),
                        },
                        onclick: {
                            let id = place.id;
                            move |_| nav.write().place = Some(id)
                        },
                    }
                }
            }
        };
    }

    if family.place_groups.is_empty() {
        return rsx! { EmptyState { icon: "storefront", title: "No places yet", line: "Start with a group below, for example Grocery Store." } };
    }
    rsx! {
        ul { class: "{CARD} overflow-hidden mt-1",
            for group in family.place_groups.values() {
                OverviewRow {
                    key: "{group.id}",
                    icon: "category",
                    name: group.name.clone(),
                    sub: match family.places_in(group.id).count() {
                        1 => "1 place".to_owned(),
                        n => format!("{n} places"),
                    },
                    count: match family.places_in(group.id).map(|place| family.open_tasks_at(place.id).count()).sum::<usize>() {
                        0 => String::new(),
                        n => format!("{n} to get"),
                    },
                    onclick: {
                        let id = group.id;
                        move |_| nav.write().group = Some(id)
                    },
                }
            }
        }
    }
}

/// Edit place: its name, its group, and its locations as a plain list. New
/// locations come from the bar at the bottom.
#[component]
fn EditPlace(place: Place) -> Element {
    let state = use_context::<AppState>();
    let mut sheet = use_context::<Signal<Option<SheetKind>>>();
    let family = (state.family)();
    let id = place.id;
    let only_one = place.locations.len() == 1;
    let mut typed = use_signal(|| place.name.clone());
    rsx! {
        label { class: "block px-1 pt-1",
            span { class: "text-sm font-semibold text-stone-700", "Name" }
            input {
                class: "{FIELD} mt-1.5",
                value: "{place.name}",
                aria_label: "Place name",
                // Saved when the field is left, or on Enter.
                onchange: move |event| rename(state, id, event.value()),
                onkeydown: move |event| {
                    if event.key() == Key::Enter {
                        rename(state, id, typed());
                    }
                },
                oninput: move |event| typed.set(event.value()),
            }
        }
        div { class: "px-1 pt-5",
            span { class: "block text-sm font-semibold text-stone-700", id: "groupLabel", "Group" }
            div { class: "mt-2 flex flex-wrap gap-2", role: "group", aria_labelledby: "groupLabel",
                for group in family.place_groups.values() {
                    button {
                        key: "{group.id}",
                        class: if group.id == place.group_id { CHIP_ON } else { CHIP_OFF },
                        aria_pressed: "{group.id == place.group_id}",
                        onclick: {
                            let group_id = group.id;
                            move |_| state.move_place(id, group_id)
                        },
                        "{group.name}"
                    }
                }
                button {
                    class: "shrink-0 inline-flex items-center gap-1 h-9 px-3.5 rounded-full text-sm font-semibold border border-dashed border-stone-500 text-stone-700",
                    onclick: move |_| sheet.set(Some(SheetKind::NewGroupFor(id))),
                    Icon { name: "add", class: "!text-[18px]" }
                    "New group"
                }
            }
        }
        Heading { text: "Locations" }
        ul { class: "{CARD} overflow-hidden",
            for location in place.locations.clone() {
                li { key: "{location.id}", class: "flex items-center gap-3 pl-4 pr-1 min-h-16",
                    Icon { name: "location_on", class: "text-stone-500" }
                    span { class: "flex-1 min-w-0",
                        span { class: "block truncate", "{location.name}" }
                        if let Some(address) = &location.address {
                            span { class: "block text-sm text-stone-600 truncate", "{address}" }
                        }
                    }
                    button {
                        class: "size-12 grid place-items-center rounded-full text-stone-600 hover:bg-stone-100 disabled:opacity-40 disabled:hover:bg-transparent",
                        aria_label: "Remove {location.name}",
                        title: if only_one { "A place needs at least one location" } else { "" },
                        disabled: only_one,
                        onclick: {
                            let location = location.clone();
                            move |_| state.remove_location(id, location.clone())
                        },
                        Icon { name: "close" }
                    }
                }
            }
        }
    }
}

fn rename(state: AppState, place: uuid::Uuid, name: String) {
    let current = state
        .family
        .peek()
        .places
        .get(&place)
        .map(|place| place.name.clone());
    if !name.trim().is_empty() && current.as_deref() != Some(name.trim()) {
        state.rename_place(place, name);
    }
}
