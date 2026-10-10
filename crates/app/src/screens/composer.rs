//! The bar at the bottom of the task list: type a task, press Enter, and the
//! bar stays ready for the next one. Above it, one-tap suggestions.

use dioxus::prelude::*;
use tackly_protocol::{Family, TaskStatus};
use uuid::Uuid;

use crate::state::AppState;

const DEFAULT_EMOJI: &str = "✅";
const MAX_SUGGESTIONS: usize = 6;

/// A task the family has finished before, offered again as a one-tap add.
#[derive(Clone, PartialEq)]
struct Suggestion {
    title: String,
    emoji: String,
    place_ids: Vec<Uuid>,
}

/// `preselected` places start ticked (the place whose page this bar is on);
/// more can be ticked before adding. Give the bar a key per place so it
/// starts over when the place changes.
#[component]
pub fn Composer(preselected: Vec<Uuid>) -> Element {
    let state = use_context::<AppState>();
    let mut title = use_signal(String::new);
    let preselected = use_signal(move || preselected);
    let mut chosen = use_signal(|| preselected.peek().clone());
    let family = (state.family)();
    let suggestions = suggestions(&family);
    // Adds the typed task. It is also what Enter does in the field.
    let mut add = move || {
        let text = title().trim().to_owned();
        if text.is_empty() {
            return;
        }
        state.add_task(text, DEFAULT_EMOJI.to_owned(), chosen());
        title.set(String::new());
        chosen.set(preselected());
    };
    rsx! {
        div { class: "composer",
            if !family.places.is_empty() {
                div { class: "suggestions",
                    for place in family.places.values() {
                        button {
                            key: "{place.id}",
                            class: "place-chip",
                            aria_pressed: "{chosen().contains(&place.id)}",
                            onclick: {
                                let id = place.id;
                                move |_| {
                                    let mut now = chosen();
                                    if now.contains(&id) {
                                        now.retain(|chosen| *chosen != id);
                                    } else {
                                        now.push(id);
                                    }
                                    chosen.set(now);
                                }
                            },
                            "{place.emoji} {place.name}"
                        }
                    }
                }
            }
            if !suggestions.is_empty() {
                div { class: "suggestions",
                    for suggestion in suggestions {
                        button {
                            key: "{suggestion.title}",
                            class: "suggestion",
                            onclick: {
                                let suggestion = suggestion.clone();
                                move |_| {
                                    // Its own places, plus the ones ticked now.
                                    let mut places = suggestion.place_ids.clone();
                                    for id in chosen() {
                                        if !places.contains(&id) {
                                            places.push(id);
                                        }
                                    }
                                    state.add_task(suggestion.title.clone(), suggestion.emoji.clone(), places);
                                }
                            },
                            span { aria_hidden: "true", "+" }
                            "{suggestion.title}"
                        }
                    }
                }
            }
            div { class: "composer-row",
                span { class: "composer-circle", aria_hidden: "true" }
                input {
                    id: "task-title",
                    aria_label: "Add a task",
                    placeholder: "Add a task",
                    value: "{title}",
                    oninput: move |event| title.set(event.value()),
                    onkeydown: move |event| {
                        if event.key() == Key::Enter {
                            add();
                        }
                    },
                }
                button {
                    class: "composer-send",
                    aria_label: "Add",
                    disabled: title().trim().is_empty(),
                    onclick: move |_| add(),
                    "↑"
                }
            }
        }
    }
}

/// Finished tasks, newest first, each title once, leaving out what is already
/// on the list.
fn suggestions(family: &Family) -> Vec<Suggestion> {
    let is_open = |title: &str| {
        family
            .tasks
            .values()
            .any(|task| !task.is_done() && task.title.eq_ignore_ascii_case(title))
    };
    let mut finished: Vec<_> = family
        .tasks
        .values()
        .filter_map(|task| match &task.status {
            TaskStatus::Done { at, .. } => Some((*at, task)),
            _ => None,
        })
        .collect();
    finished.sort_by_key(|(at, _)| std::cmp::Reverse(*at));

    let mut seen: Vec<String> = Vec::new();
    let mut result = Vec::new();
    for (_, task) in finished {
        let key = task.title.to_lowercase();
        if seen.contains(&key) || is_open(&task.title) {
            continue;
        }
        seen.push(key);
        result.push(Suggestion {
            title: task.title.clone(),
            emoji: task.emoji.clone(),
            place_ids: task
                .place_ids
                .iter()
                .filter(|id| family.places.contains_key(id))
                .copied()
                .collect(),
        });
        if result.len() == MAX_SUGGESTIONS {
            break;
        }
    }
    result
}
