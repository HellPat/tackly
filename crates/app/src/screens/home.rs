//! The frame around a family: app bar, the four tabs, the add-a-task bar and
//! the sheets.

use dioxus::prelude::*;
use uuid::Uuid;

use super::{
    activity::ActivityTab,
    composer::Composer,
    family::FamilyTab,
    place_sheets::{AddLocationSheet, NewGroupSheet, NewPlaceSheet, TaskPlacesSheet},
    places::{PlaceView, PlacesTab},
    sheets::FinishSheet,
    tasks::TasksTab,
};
use crate::state::AppState;

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Tasks,
    Places,
    Activity,
    Family,
}

/// What is open over the tabs.
#[derive(Clone, PartialEq)]
enum Sheet {
    None,
    Finish(Uuid),
    NewGroup,
    NewPlace(Uuid),
    AddLocation(Uuid),
    TaskPlaces(Uuid),
}

#[component]
pub fn Home() -> Element {
    let mut tab = use_signal(|| Tab::Tasks);
    let mut sheet = use_signal(|| Sheet::None);
    // The place being looked at in the Places tab.
    let mut place = use_signal(|| None::<Uuid>);
    rsx! {
        AppBar {}
        div { class: "content",
            match tab() {
                Tab::Tasks => rsx! {
                    TasksTab {
                        open_finish: move |task| sheet.set(Sheet::Finish(task)),
                        edit_places: move |task| sheet.set(Sheet::TaskPlaces(task)),
                    }
                },
                Tab::Places => match place() {
                    Some(id) => rsx! {
                        PlaceView {
                            place: id,
                            back: move |_| place.set(None),
                            add_location: move |id| sheet.set(Sheet::AddLocation(id)),
                            open_finish: move |task| sheet.set(Sheet::Finish(task)),
                            edit_places: move |task| sheet.set(Sheet::TaskPlaces(task)),
                        }
                    },
                    None => rsx! {
                        PlacesTab {
                            open_place: move |id| place.set(Some(id)),
                            new_group: move |_| sheet.set(Sheet::NewGroup),
                            new_place: move |group| sheet.set(Sheet::NewPlace(group)),
                        }
                    },
                },
                Tab::Activity => rsx! { ActivityTab {} },
                Tab::Family => rsx! { FamilyTab {} },
            }
        }
        if tab() == Tab::Tasks {
            Composer { preselected: Vec::new() }
        }
        if let (Tab::Places, Some(id)) = (tab(), place()) {
            Composer { key: "{id}", preselected: vec![id] }
        }
        div { class: "nav",
            NavButton { icon: "✓", label: "Tasks", active: tab() == Tab::Tasks, onclick: move |_| tab.set(Tab::Tasks) }
            NavButton { icon: "📍", label: "Places", active: tab() == Tab::Places, onclick: move |_| {
                tab.set(Tab::Places);
                place.set(None);
            } }
            NavButton { icon: "🕘", label: "Activity", active: tab() == Tab::Activity, onclick: move |_| tab.set(Tab::Activity) }
            NavButton { icon: "👪", label: "Family", active: tab() == Tab::Family, onclick: move |_| tab.set(Tab::Family) }
        }
        match sheet() {
            Sheet::None => rsx! {},
            Sheet::NewGroup => rsx! { NewGroupSheet { close: move |_| sheet.set(Sheet::None) } },
            Sheet::NewPlace(group) => rsx! { NewPlaceSheet { group, close: move |_| sheet.set(Sheet::None) } },
            Sheet::AddLocation(id) => rsx! { AddLocationSheet { place: id, close: move |_| sheet.set(Sheet::None) } },
            Sheet::TaskPlaces(task) => rsx! { TaskPlacesSheet { task, close: move |_| sheet.set(Sheet::None) } },
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
