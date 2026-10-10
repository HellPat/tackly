//! The frame around a family: app bar, the three tabs, the add-a-task bar and
//! the sheets.

use dioxus::prelude::*;
use uuid::Uuid;

use super::{
    activity::ActivityTab, composer::Composer, family::FamilyTab, sheets::FinishSheet,
    tasks::TasksTab,
};
use crate::state::AppState;

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Tasks,
    Activity,
    Family,
}

/// What is open over the tabs.
#[derive(Clone, PartialEq)]
enum Sheet {
    None,
    Finish(Uuid),
}

#[component]
pub fn Home() -> Element {
    let mut tab = use_signal(|| Tab::Tasks);
    let mut sheet = use_signal(|| Sheet::None);
    rsx! {
        AppBar {}
        div { class: "content",
            match tab() {
                Tab::Tasks => rsx! {
                    TasksTab { open_finish: move |task| sheet.set(Sheet::Finish(task)) }
                },
                Tab::Activity => rsx! { ActivityTab {} },
                Tab::Family => rsx! { FamilyTab {} },
            }
        }
        if tab() == Tab::Tasks {
            Composer {}
        }
        div { class: "nav",
            NavButton { icon: "✓", label: "Tasks", active: tab() == Tab::Tasks, onclick: move |_| tab.set(Tab::Tasks) }
            NavButton { icon: "🕘", label: "Activity", active: tab() == Tab::Activity, onclick: move |_| tab.set(Tab::Activity) }
            NavButton { icon: "👪", label: "Family", active: tab() == Tab::Family, onclick: move |_| tab.set(Tab::Family) }
        }
        match sheet() {
            Sheet::None => rsx! {},
            Sheet::Finish(task) => rsx! { FinishSheet { task, close: move |_| sheet.set(Sheet::None) } },
        }
    }
}

/// The family's name, its size, and whether the relay is reachable.
#[component]
fn AppBar() -> Element {
    let state = use_context::<AppState>();
    let family = (state.family)();
    let name = family.name.clone().unwrap_or_else(|| "Tackly".into());
    let shared_yet = (state.membership)().is_none_or(|membership| membership.registered);
    rsx! {
        div { class: "app-bar",
            div { style: "flex:1",
                h1 { "Tackly 🌱" }
                div { class: "sub", "{name} · {family.members.len()} members" }
            }
            if (state.online)() {
                span { class: "sync on",
                    i {}
                    "Live"
                }
            } else {
                span { class: "sync off",
                    i {}
                    if shared_yet { "Offline · will sync" } else { "Offline · not shared yet" }
                }
            }
        }
    }
}

#[component]
fn NavButton(
    icon: String,
    label: String,
    active: bool,
    onclick: EventHandler<MouseEvent>,
) -> Element {
    rsx! {
        button { class: if active { "active" } else { "" }, onclick: move |event| onclick.call(event),
            div { class: "pill", aria_hidden: "true", "{icon}" }
            "{label}"
        }
    }
}
