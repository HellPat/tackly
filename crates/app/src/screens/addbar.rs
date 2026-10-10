//! The bar at the bottom: the main action of the screen above it. It adds a
//! task (to the list you are in, or to "Other"), a group, a place, or a
//! location. For places and locations it searches addresses while you type.

use std::time::Duration;

use dioxus::prelude::*;
use tackly_protocol::PlaceLocation;
use uuid::Uuid;

use crate::{
    state::AppState,
    ui::{CARD, Icon},
};

/// How long typing must pause before the address search starts.
const SEARCH_PAUSE: Duration = Duration::from_millis(300);
const MIN_SEARCH_LETTERS: usize = 3;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Mode {
    /// A task, in a list (`None`: "Other"), needed at a place if given.
    Task {
        list: Option<Uuid>,
        place: Option<Uuid>,
    },
    Group,
    Place {
        group: Uuid,
    },
    Location {
        place: Uuid,
    },
}

impl Mode {
    fn label(self) -> &'static str {
        match self {
            Mode::Task { .. } => "Add a task",
            Mode::Group => "Add a group",
            Mode::Place { .. } => "Add a place",
            Mode::Location { .. } => "Add a location",
        }
    }

    fn searches_addresses(self) -> bool {
        matches!(self, Mode::Place { .. } | Mode::Location { .. })
    }
}

#[component]
pub fn AddBar(mode: Mode) -> Element {
    let state = use_context::<AppState>();
    let mut text = use_signal(String::new);
    let mut found = use_signal(Vec::<PlaceLocation>::new);
    let mut searched = use_signal(|| 0_u64);
    let label = mode.label();

    // Adds what was typed, or the address that was picked.
    let mut add = move |picked: Option<PlaceLocation>| {
        let typed = text().trim().to_owned();
        let name = picked
            .as_ref()
            .map_or(typed.clone(), |location| location.name.clone());
        if name.is_empty() {
            return;
        }
        let location = picked.unwrap_or_else(|| PlaceLocation::named(name.clone()));
        match mode {
            Mode::Task { list, place } => state.add_task(name, list, place.into_iter().collect()),
            Mode::Group => state.create_group(name),
            Mode::Place { group } => state.create_place(group, name, location),
            Mode::Location { place } => state.add_location(place, location),
        }
        text.set(String::new());
        found.set(Vec::new());
    };

    rsx! {
        section { class: "relative z-10 shrink-0 px-3 pb-3 pt-2 flex flex-col gap-2", aria_label: "Adding",
            if !found().is_empty() {
                ul { class: "{CARD} shadow-md overflow-hidden", aria_label: "Addresses",
                    for option in found() {
                        li { key: "{option.id}",
                            button {
                                r#type: "button",
                                class: "w-full flex items-center gap-3 px-4 min-h-14 text-left hover:bg-stone-50",
                                onclick: {
                                    let option = option.clone();
                                    move |_| add(Some(option.clone()))
                                },
                                Icon { name: "location_on", class: "text-stone-500" }
                                span {
                                    span { class: "block", "{option.name}" }
                                    if let Some(address) = &option.address {
                                        span { class: "block text-sm text-stone-600", "{address}" }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            // No form: Enter adds (below), so it can never add twice.
            div { class: "flex items-center gap-2",
                label { class: "flex-1 flex items-center gap-3 h-12 pl-2 pr-4 rounded-full bg-[#fffdfb] ring-1 ring-stone-300 shadow-sm focus-within:ring-2 focus-within:ring-accent-700",
                    span { class: "size-8 shrink-0 rounded-full grid place-items-center bg-accent-700 text-white", aria_hidden: "true",
                        Icon { name: "add", class: "!text-[20px]" }
                    }
                    input {
                        class: "flex-1 min-w-0 border-0 bg-transparent p-0 text-base outline-none placeholder:text-stone-500",
                        aria_label: "{label}",
                        placeholder: "{label}",
                        autocomplete: "off",
                        value: "{text}",
                        onkeydown: move |event| {
                            if event.key() == Key::Enter {
                                add(None);
                            }
                        },
                        oninput: move |event| {
                            let typed = event.value();
                            text.set(typed.clone());
                            if !mode.searches_addresses() {
                                return;
                            }
                            // Only the latest keystroke's search may show its result.
                            let this_search = searched() + 1;
                            searched.set(this_search);
                            let typed = typed.trim().to_owned();
                            if typed.chars().count() < MIN_SEARCH_LETTERS {
                                found.set(Vec::new());
                                return;
                            }
                            let geocoder = state.geocoder.peek().clone();
                            spawn(async move {
                                tokio::time::sleep(SEARCH_PAUSE).await;
                                if searched() != this_search {
                                    return;
                                }
                                // Offline or no match: what was typed is kept as a name.
                                let result = geocoder.search(&typed).await.unwrap_or_default();
                                if searched() == this_search {
                                    found.set(result);
                                }
                            });
                        },
                    }
                }
                if !text().trim().is_empty() {
                    button {
                        aria_label: "Add",
                        onclick: move |_| add(None),
                        class: "size-12 shrink-0 grid place-items-center rounded-md border border-accent-800 bg-accent-700 text-white shadow-sm inset-shadow-[0_1px_0_rgb(255_255_255/.18)] hover:bg-accent-800",
                        Icon { name: "arrow_upward" }
                    }
                }
            }
        }
    }
}
