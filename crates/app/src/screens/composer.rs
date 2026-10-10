//! The bar at the bottom of the task list: type a task, press Enter, and the
//! bar stays ready for the next one. Above it, one-tap suggestions.

use dioxus::prelude::*;
use tackly_protocol::{Family, TaskStatus};

use crate::state::AppState;

const DEFAULT_EMOJI: &str = "✅";
const MAX_SUGGESTIONS: usize = 6;

/// A task the family has finished before, offered again as a one-tap add.
#[derive(Clone, PartialEq)]
struct Suggestion {
    title: String,
    emoji: String,
}

#[component]
pub fn Composer() -> Element {
    let state = use_context::<AppState>();
    let mut title = use_signal(String::new);
    let suggestions = suggestions(&(state.family)());
    // Adds the typed task. It is also what Enter does in the field.
    let mut add = move || {
        let text = title().trim().to_owned();
        if text.is_empty() {
            return;
        }
        state.add_task(text, DEFAULT_EMOJI.to_owned());
        title.set(String::new());
    };
    rsx! {
        div { class: "composer",
            if !suggestions.is_empty() {
                div { class: "suggestions",
                    for suggestion in suggestions {
                        button {
                            key: "{suggestion.title}",
                            class: "suggestion",
                            onclick: {
                                let suggestion = suggestion.clone();
                                move |_| state.add_task(suggestion.title.clone(), suggestion.emoji.clone())
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
        });
        if result.len() == MAX_SUGGESTIONS {
            break;
        }
    }
    result
}
