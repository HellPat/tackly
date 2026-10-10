//! The forms that slide up over the task list.

use dioxus::prelude::*;
use uuid::Uuid;

use crate::{platform, state::AppState};

const EMOJIS: [&str; 10] = ["✅", "🍽", "🧺", "🗑", "🛒", "🧹", "🪴", "🐕", "🚗", "📞"];

/// A sheet over a dimmed screen: tapping outside closes it, tapping inside
/// does not.
#[component]
fn Sheet(close: EventHandler<()>, children: Element) -> Element {
    rsx! {
        div { class: "scrim", onclick: move |_| close.call(()),
            div { class: "sheet", onclick: move |event| event.stop_propagation(),
                div { class: "handle" }
                {children}
            }
        }
    }
}

#[component]
pub fn AddTaskSheet(close: EventHandler<()>) -> Element {
    let state = use_context::<AppState>();
    let mut title = use_signal(String::new);
    let mut emoji = use_signal(|| EMOJIS[0].to_owned());
    // Adds the task. It is also what Enter does in the title field.
    let add = move || {
        if title().trim().is_empty() {
            return;
        }
        state.add_task(title(), emoji());
        close.call(());
    };
    rsx! {
        Sheet { close,
            h2 { "New task" }
            div { class: "field",
                label { r#for: "task-title", "What needs doing?" }
                input {
                    id: "task-title",
                    value: "{title}",
                    autofocus: true,
                    oninput: move |event| title.set(event.value()),
                    onkeydown: move |event| {
                        if event.key() == Key::Enter {
                            add();
                        }
                    },
                }
            }
            div { class: "emoji-row",
                for choice in EMOJIS {
                    button {
                        key: "{choice}",
                        class: if emoji() == choice { "sel" } else { "" },
                        onclick: move |_| emoji.set(choice.to_owned()),
                        "{choice}"
                    }
                }
            }
            div { class: "row",
                button { class: "btn text", onclick: move |_| close.call(()), "Cancel" }
                button {
                    class: "btn",
                    disabled: title().trim().is_empty(),
                    onclick: move |_| add(),
                    "Add"
                }
            }
        }
    }
}

#[component]
pub fn FinishSheet(task: Uuid, close: EventHandler<()>) -> Element {
    let state = use_context::<AppState>();
    let mut note = use_signal(String::new);
    let mut with_location = use_signal(|| true);
    // Only offered where the phone can tell where it is.
    let can_locate = platform::location().is_some();
    let title = (state.family)()
        .tasks
        .get(&task)
        .map(|task| task.title.clone())
        .unwrap_or_default();
    rsx! {
        Sheet { close,
            h2 { "Finish “{title}”" }
            div { class: "field",
                label { r#for: "task-note", "Note (optional)" }
                input {
                    id: "task-note",
                    value: "{note}",
                    oninput: move |event| note.set(event.value()),
                }
            }
            if can_locate {
                label { class: "switch",
                    input {
                        r#type: "checkbox",
                        checked: with_location(),
                        onchange: move |event| with_location.set(event.checked()),
                    }
                    "Add where I am"
                }
            }
            div { class: "row",
                button { class: "btn text", onclick: move |_| close.call(()), "Cancel" }
                button {
                    class: "btn",
                    onclick: move |_| {
                        let location = if with_location() { platform::location() } else { None };
                        state.complete_task(task, note(), location);
                        close.call(());
                    },
                    "Done"
                }
            }
        }
    }
}
