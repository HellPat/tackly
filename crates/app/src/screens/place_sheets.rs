//! Forms for places: a new group, a new place, another location for a place,
//! and which places a task belongs to.

use std::time::Duration;

use dioxus::prelude::*;
use tackly_protocol::PlaceLocation;
use uuid::Uuid;

use super::sheets::Sheet;
use crate::state::AppState;

const EMOJIS: [&str; 10] = ["🛒", "🏪", "🥦", "💊", "🔨", "🐾", "⛽", "📦", "🏠", "🏥"];
/// How long typing must pause before the address search starts.
const SEARCH_PAUSE: Duration = Duration::from_millis(300);
const MIN_SEARCH_LETTERS: usize = 3;

#[component]
fn EmojiRow(chosen: Signal<String>) -> Element {
    let mut chosen = chosen;
    rsx! {
        div { class: "emoji-row",
            for choice in EMOJIS {
                button {
                    key: "{choice}",
                    class: if chosen() == choice { "sel" } else { "" },
                    onclick: move |_| chosen.set(choice.to_owned()),
                    "{choice}"
                }
            }
        }
    }
}

#[component]
pub fn NewGroupSheet(close: EventHandler<()>) -> Element {
    let state = use_context::<AppState>();
    let mut name = use_signal(String::new);
    let emoji = use_signal(|| EMOJIS[0].to_owned());
    rsx! {
        Sheet { close,
            h2 { "New group" }
            div { class: "field",
                label { r#for: "group-name", "Group name, e.g. Grocery Store" }
                input {
                    id: "group-name",
                    value: "{name}",
                    oninput: move |event| name.set(event.value()),
                }
            }
            EmojiRow { chosen: emoji }
            div { class: "row",
                button { class: "btn text", onclick: move |_| close.call(()), "Cancel" }
                button {
                    class: "btn",
                    disabled: name().trim().is_empty(),
                    onclick: move |_| {
                        state.create_place_group(name(), emoji());
                        close.call(());
                    },
                    "Create group"
                }
            }
        }
    }
}

#[component]
pub fn NewPlaceSheet(group: Uuid, close: EventHandler<()>) -> Element {
    let state = use_context::<AppState>();
    let mut name = use_signal(String::new);
    let emoji = use_signal(|| EMOJIS[1].to_owned());
    let mut location = use_signal(|| None::<PlaceLocation>);
    let group_name = (state.family)()
        .place_groups
        .get(&group)
        .map(|group| group.name.clone())
        .unwrap_or_default();
    rsx! {
        Sheet { close,
            h2 { "New place in {group_name}" }
            div { class: "field",
                label { r#for: "place-name", "Place name, e.g. LIDL" }
                input {
                    id: "place-name",
                    value: "{name}",
                    oninput: move |event| name.set(event.value()),
                }
            }
            EmojiRow { chosen: emoji }
            LocationPicker { on_change: move |picked| location.set(picked) }
            div { class: "row",
                button { class: "btn text", onclick: move |_| close.call(()), "Cancel" }
                button {
                    class: "btn",
                    disabled: name().trim().is_empty() || location().is_none(),
                    onclick: move |_| {
                        if let Some(location) = location() {
                            state.create_place(group, name(), emoji(), location);
                            close.call(());
                        }
                    },
                    "Create place"
                }
            }
        }
    }
}

#[component]
pub fn AddLocationSheet(place: Uuid, close: EventHandler<()>) -> Element {
    let state = use_context::<AppState>();
    let mut location = use_signal(|| None::<PlaceLocation>);
    let name = (state.family)()
        .places
        .get(&place)
        .map(|place| place.name.clone())
        .unwrap_or_default();
    rsx! {
        Sheet { close,
            h2 { "Another {name}" }
            LocationPicker { on_change: move |picked| location.set(picked) }
            div { class: "row",
                button { class: "btn text", onclick: move |_| close.call(()), "Cancel" }
                button {
                    class: "btn",
                    disabled: location().is_none(),
                    onclick: move |_| {
                        if let Some(location) = location() {
                            state.add_place_location(place, location);
                            close.call(());
                        }
                    },
                    "Add location"
                }
            }
        }
    }
}

/// A text field that suggests addresses while typing ("LIDL Winnenden").
/// With no connection, or no match, what was typed is kept as a plain name.
#[component]
fn LocationPicker(on_change: EventHandler<Option<PlaceLocation>>) -> Element {
    let state = use_context::<AppState>();
    let mut text = use_signal(String::new);
    let mut found = use_signal(Vec::<PlaceLocation>::new);
    let mut searched = use_signal(|| 0_u64);
    rsx! {
        div { class: "field",
            label { r#for: "location-text", "Location, e.g. LIDL Winnenden" }
            input {
                id: "location-text",
                value: "{text}",
                oninput: move |event| {
                    let typed = event.value();
                    text.set(typed.clone());
                    let typed = typed.trim().to_owned();
                    on_change.call((!typed.is_empty()).then(|| PlaceLocation::named(typed.clone())));
                    // Only the latest keystroke's search may show its result.
                    let this_search = searched() + 1;
                    searched.set(this_search);
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
                        let result = geocoder.search(&typed).await.unwrap_or_default();
                        if searched() == this_search {
                            found.set(result);
                        }
                    });
                },
            }
        }
        if !found().is_empty() {
            div { class: "suggest-list",
                for option in found() {
                    button {
                        key: "{option.id}",
                        class: "suggest-item",
                        onclick: {
                            let option = option.clone();
                            move |_| {
                                text.set(option.name.clone());
                                found.set(Vec::new());
                                on_change.call(Some(option.clone()));
                            }
                        },
                        span { class: "suggest-name", "{option.name}" }
                        if let Some(address) = &option.address {
                            span { class: "suggest-address", "{address}" }
                        }
                    }
                }
            }
        }
    }
}

/// Ticking the places a task belongs to.
#[component]
pub fn TaskPlacesSheet(task: Uuid, close: EventHandler<()>) -> Element {
    let state = use_context::<AppState>();
    let family = (state.family)();
    let Some(current) = family.tasks.get(&task) else {
        return rsx! {};
    };
    rsx! {
        Sheet { close,
            h2 { "Where can “{current.title}” be done?" }
            for place in family.places.values() {
                label { key: "{place.id}", class: "switch",
                    input {
                        r#type: "checkbox",
                        checked: current.place_ids.contains(&place.id),
                        onchange: {
                            let place_id = place.id;
                            move |event| state.set_task_at_place(task, place_id, event.checked())
                        },
                    }
                    "{place.emoji} {place.name}"
                }
            }
            div { class: "row",
                button { class: "btn", onclick: move |_| close.call(()), "Done" }
            }
        }
    }
}
