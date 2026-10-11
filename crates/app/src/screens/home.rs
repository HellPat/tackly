//! The frame around a family: the color scheme and its drawing, the top bar
//! (back, title, edit, offline), the tab's content, the bar at the bottom, the
//! navigation, toasts, and the sheets. [`Nav`] says where you are.

use dioxus::prelude::*;
use uuid::Uuid;

use super::{
    addbar::{AddBar, Mode},
    detail::Detail,
    family::FamilyTab,
    places::PlacesTab,
    sheets::{AssignSheet, InviteSheet, NameSheet},
    tasks::TasksTab,
};
use crate::{
    state::AppState,
    theme,
    ui::{FILLED, Icon, PRIMARY, Toasts},
};

#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum Tab {
    #[default]
    Tasks,
    Places,
    Family,
}

/// Where you are. Each tab is an overview you can step into.
#[derive(Clone, Copy, PartialEq, Default, Debug)]
pub struct Nav {
    pub tab: Tab,
    /// Tasks: the list you stepped into.
    pub list: Option<Uuid>,
    /// Places: the group, then the place you stepped into, and whether it is being edited.
    pub group: Option<Uuid>,
    pub place: Option<Uuid>,
    pub editing_place: bool,
    /// Family: the person you opened; your own entry opens your settings.
    pub person: Option<Uuid>,
    pub settings: bool,
    /// A task in focus, full screen.
    pub detail: Option<Uuid>,
}

impl Nav {
    fn back(&mut self) {
        if self.settings || self.person.is_some() {
            (self.settings, self.person) = (false, None);
        } else if self.editing_place {
            self.editing_place = false;
        } else if self.place.is_some() {
            self.place = None;
        } else if self.group.is_some() {
            self.group = None;
        } else {
            self.list = None;
        }
    }

    fn can_go_back(&self) -> bool {
        match self.tab {
            Tab::Tasks => self.list.is_some(),
            Tab::Places => self.group.is_some() || self.place.is_some(),
            Tab::Family => self.person.is_some() || self.settings,
        }
    }
}

/// The small dialogs, one at a time.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum SheetKind {
    NewList,
    RenameList(Uuid),
    RenameGroup(Uuid),
    /// A new group, made from Edit place: that place moves into it.
    NewGroupFor(Uuid),
    Assign(Uuid),
    Invite,
}

#[component]
pub fn Home() -> Element {
    let state = use_context::<AppState>();
    let mut nav = use_context_provider(|| Signal::new(Nav::default()));
    let mut sheet = use_context_provider(|| Signal::new(None::<SheetKind>));
    let family = (state.family)();
    let here = nav();
    let scheme = theme::scheme(&(state.settings)().scheme);
    // Right after creating or joining a family, so finishing a task never
    // stops for a permission dialog.
    use_hook(crate::platform::ask_for_location);

    let title = match here.tab {
        Tab::Tasks => here
            .list
            .and_then(|id| family.lists.get(&id))
            .map_or("Tasks".into(), |list| list.name.clone()),
        Tab::Places => match (here.place, here.group) {
            (Some(_), _) if here.editing_place => "Edit place".into(),
            (Some(id), _) => family
                .places
                .get(&id)
                .map_or_else(String::new, |place| place.name.clone()),
            (None, Some(id)) => family
                .place_groups
                .get(&id)
                .map_or_else(String::new, |group| group.name.clone()),
            (None, None) => "Places".into(),
        },
        Tab::Family if here.settings => "Your settings".into(),
        Tab::Family => here.person.map_or_else(
            || family.name.clone().unwrap_or_else(|| "Family".into()),
            |id| family.member_name(id).to_owned(),
        ),
    };
    // Edit lives in the top bar: for a list, a group, and a place.
    let edit = match here.tab {
        Tab::Tasks => here.list.map(SheetKind::RenameList).map(Edit::Sheet),
        Tab::Places if here.place.is_some() && !here.editing_place => Some(Edit::Place),
        Tab::Places if here.place.is_none() => {
            here.group.map(SheetKind::RenameGroup).map(Edit::Sheet)
        }
        _ => None,
    };
    let bar = match here.tab {
        Tab::Tasks => Some(Mode::Task {
            list: here.list,
            place: None,
        }),
        Tab::Places if here.editing_place => here.place.map(|place| Mode::Location { place }),
        Tab::Places => match (here.place, here.group) {
            (Some(place), _) => Some(Mode::Task {
                list: None,
                place: Some(place),
            }),
            (None, Some(group)) => Some(Mode::Place { group }),
            (None, None) => Some(Mode::Group),
        },
        Tab::Family => None,
    };
    let is_head = (state.membership)().is_some_and(|membership| membership.owner);
    let show_invite = here.tab == Tab::Family && here.person.is_none() && !here.settings && is_head;

    rsx! {
        div { class: "relative h-full flex flex-col overflow-hidden text-stone-800 {scheme.sky}", style: "{scheme.accent_vars()}",
            div {
                class: "absolute inset-x-0 bottom-20 h-[42%] pointer-events-none",
                aria_hidden: "true",
                dangerous_inner_html: "{scheme.scene_svg()}",
            }
            header { class: "relative z-10 shrink-0 flex items-center gap-1 px-2 h-16",
                if here.can_go_back() {
                    button {
                        class: "size-12 grid place-items-center rounded-full text-stone-700 hover:bg-stone-900/5",
                        aria_label: "Back",
                        onclick: move |_| nav.write().back(),
                        Icon { name: "arrow_back" }
                    }
                }
                h1 { class: "flex-1 px-2 text-2xl font-semibold tracking-tight truncate", "{title}" }
                if let Some(edit) = edit {
                    button {
                        class: "size-12 grid place-items-center rounded-full text-stone-700 hover:bg-stone-900/5",
                        aria_label: "Edit",
                        onclick: move |_| match edit {
                            Edit::Sheet(kind) => sheet.set(Some(kind)),
                            Edit::Place => nav.write().editing_place = true,
                        },
                        Icon { name: "edit" }
                    }
                }
                if !(state.online)() {
                    span { class: "mr-2 inline-flex items-center gap-1 h-8 px-3 rounded-full bg-stone-900/10 text-sm text-stone-700",
                        Icon { name: "cloud_off", class: "!text-[18px]" }
                        "Offline"
                    }
                }
            }
            main { class: "relative z-10 flex-1 overflow-y-auto px-3 pb-6",
                match here.tab {
                    Tab::Tasks => rsx! { TasksTab {} },
                    Tab::Places => rsx! { PlacesTab {} },
                    Tab::Family => rsx! { FamilyTab {} },
                }
            }
            if let Some(mode) = bar {
                AddBar { key: "{mode:?}", mode }
            }
            if show_invite {
                section { class: "relative z-10 shrink-0 px-3 pb-3 pt-2", aria_label: "Invite",
                    button { class: "{PRIMARY} w-full !h-12 text-base", onclick: move |_| sheet.set(Some(SheetKind::Invite)),
                        Icon { name: "person_add" }
                        "Invite member"
                    }
                }
            }
            nav { class: "relative z-10 shrink-0 h-20 grid grid-cols-3 bg-[#fffdfb] shadow-[0_-1px_0_var(--color-stone-200)]", aria_label: "Main",
                TabButton { tab: Tab::Tasks, icon: "task_alt", label: "Tasks" }
                TabButton { tab: Tab::Places, icon: "storefront", label: "Places" }
                TabButton { tab: Tab::Family, icon: "group", label: "Family" }
            }
            Toasts {}
            if let Some(task) = here.detail {
                Detail { task }
            }
            match sheet() {
                Some(SheetKind::NewList) => rsx! { NameSheet { kind: SheetKind::NewList } },
                Some(kind @ (SheetKind::RenameList(_) | SheetKind::RenameGroup(_) | SheetKind::NewGroupFor(_))) => rsx! { NameSheet { kind } },
                Some(SheetKind::Assign(task)) => rsx! { AssignSheet { task } },
                Some(SheetKind::Invite) => rsx! { InviteSheet {} },
                None => rsx! {},
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Edit {
    Sheet(SheetKind),
    Place,
}

/// A tab in the navigation: the active one has a tinted pill and a filled icon.
#[component]
fn TabButton(tab: Tab, icon: String, label: String) -> Element {
    let mut nav = use_context::<Signal<Nav>>();
    let active = nav().tab == tab;
    rsx! {
        button {
            class: "flex flex-col items-center justify-center gap-1",
            aria_current: if active { "page" } else { "false" },
            onclick: move |_| nav.set(Nav { tab, ..Nav::default() }),
            span { class: if active { "w-16 h-8 rounded-full grid place-items-center transition-colors bg-accent-100 text-accent-900" } else { "w-16 h-8 rounded-full grid place-items-center transition-colors text-stone-600" },
                Icon { name: icon, class: if active { FILLED } else { "" } }
            }
            span { class: if active { "text-xs font-semibold text-stone-900" } else { "text-xs text-stone-600" }, "{label}" }
        }
    }
}
