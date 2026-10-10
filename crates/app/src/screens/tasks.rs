//! The Tasks tab: your lists to step into, "Other" shown open below them, and
//! the task card used everywhere.

use dioxus::prelude::*;
use tackly_protocol::{Progress, Task};
use uuid::Uuid;

use super::home::{Nav, SheetKind};
use crate::{
    settings::Filter,
    state::AppState,
    ui::{Avatar, CARD, EmptyState, FilterBar, Icon, OverviewRow, SECONDARY, Size},
};

/// Whether a task passes the filter, for this phone's person.
pub fn keep(filter: Filter, task: &Task, me: Uuid) -> bool {
    match filter {
        Filter::Mine => task.assignee == Some(me),
        Filter::Unassigned => task.assignee.is_none(),
        Filter::All => true,
    }
}

/// How many tasks each filter shows.
pub fn counts<'a>(tasks: impl IntoIterator<Item = &'a Task>, me: Uuid) -> [usize; 3] {
    tasks.into_iter().fold([0; 3], |mut n, task| {
        for (i, filter) in [Filter::Mine, Filter::Unassigned, Filter::All]
            .into_iter()
            .enumerate()
        {
            n[i] += usize::from(keep(filter, task, me));
        }
        n
    })
}

fn filter_word(filter: Filter) -> &'static str {
    match filter {
        Filter::Mine => "yours",
        Filter::Unassigned => "unassigned",
        Filter::All => "open",
    }
}

#[component]
pub fn TasksTab() -> Element {
    let state = use_context::<AppState>();
    let mut nav = use_context::<Signal<Nav>>();
    let mut sheet = use_context::<Signal<Option<SheetKind>>>();
    let (family, me, filter) = ((state.family)(), (state.my_id)(), (state.settings)().filter);

    if let Some(list) = nav().list {
        let tasks: Vec<Task> = family.open_tasks_in(Some(list)).cloned().collect();
        return rsx! { TaskList { tasks, empty_line: "Nothing left to do. Enjoy it." } };
    }

    let all: Vec<&Task> = family.open_tasks().collect();
    let other: Vec<Task> = family
        .open_tasks_in(None)
        .filter(|task| keep(filter, task, me))
        .cloned()
        .collect();
    rsx! {
        FilterBar { counts: counts(all.iter().copied(), me) }
        if !family.lists.is_empty() {
            ul { class: "{CARD} overflow-hidden",
                for list in family.lists.values() {
                    OverviewRow {
                        key: "{list.id}",
                        icon: "checklist",
                        name: list.name.clone(),
                        count: match family.open_tasks_in(Some(list.id)).filter(|task| keep(filter, task, me)).count() {
                            0 => String::new(),
                            n => format!("{n} {}", filter_word(filter)),
                        },
                        onclick: {
                            let id = list.id;
                            move |_| nav.write().list = Some(id)
                        },
                    }
                }
            }
        }
        // "Other" looks like a list, but is always open. Nothing to show: no card.
        if !other.is_empty() {
            section { class: "{CARD} overflow-hidden mt-4", aria_label: "Other",
                div { class: "flex items-center gap-3 px-4 min-h-14",
                    span { class: "size-9 shrink-0 rounded-full grid place-items-center bg-stone-100 text-stone-600",
                        Icon { name: "checklist", class: "!text-[18px]" }
                    }
                    h2 { class: "flex-1 text-base", "Other" }
                    span { class: "text-sm text-stone-600", "{other.len()} {filter_word(filter)}" }
                    Icon { name: "expand_less", class: "text-stone-500" }
                }
                ul { class: "pb-1",
                    for task in other {
                        TaskCard { key: "{task.id}", task, flat: true }
                    }
                }
            }
        }
        if all.is_empty() && family.lists.is_empty() {
            EmptyState { icon: "celebration", title: "All done", line: "Nothing left to do. Add a task below." }
        }
        div { class: "mt-3 flex justify-center",
            button { class: SECONDARY, onclick: move |_| sheet.set(Some(SheetKind::NewList)),
                Icon { name: "add", class: "!text-[18px]" }
                "New list"
            }
        }
    }
}

/// The tasks of one list or one place, with the filter on top.
#[component]
pub fn TaskList(tasks: Vec<Task>, empty_line: String) -> Element {
    let state = use_context::<AppState>();
    let (me, filter) = ((state.my_id)(), (state.settings)().filter);
    if tasks.is_empty() {
        return rsx! { EmptyState { icon: "celebration", title: "All done", line: empty_line } };
    }
    let shown: Vec<Task> = tasks
        .iter()
        .filter(|task| keep(filter, task, me))
        .cloned()
        .collect();
    rsx! {
        FilterBar { counts: counts(&tasks, me) }
        if shown.is_empty() {
            p { class: "py-16 text-center text-stone-600",
                match filter {
                    Filter::Mine => "Nothing of yours here.",
                    _ => "Every task here has someone on it.",
                }
            }
        } else {
            ul {
                for task in shown {
                    TaskCard { key: "{task.id}", task }
                }
            }
        }
    }
}

/// A task. Left: the finish circle, or the state of whoever else has it.
/// Middle: the title (opens it). Right: who has it, or a placeholder to give it to someone.
#[component]
pub fn TaskCard(task: Task, #[props(default)] flat: bool) -> Element {
    let state = use_context::<AppState>();
    let mut nav = use_context::<Signal<Nav>>();
    let mut sheet = use_context::<Signal<Option<SheetKind>>>();
    let (family, me) = ((state.family)(), (state.my_id)());
    // 0: shown; 1: sliding away; 2: folding up (then it is finished).
    let mut leaving = use_signal(|| 0_u8);
    let id = task.id;
    let holder = task
        .assignee
        .and_then(|who| family.members.get(&who).cloned());
    let mine = task.assignee == Some(me);
    let taken = task.assignee.is_some() && !mine;
    let progress = task.progress();
    let state_words = holder.as_ref().zip(progress).map(|(who, progress)| {
        let name = if mine {
            "You".to_owned()
        } else {
            who.name.clone()
        };
        match progress {
            Progress::Working if mine => "You’re working on it".to_owned(),
            Progress::Working => format!("{name} is working on it"),
            Progress::Paused => format!("{name} paused it"),
            Progress::Picked if task.assigned_by.is_some_and(|by| Some(by) != task.assignee) => {
                format!("Assigned to {}", if mine { "you" } else { &who.name })
            }
            Progress::Picked => format!("{name} picked it"),
        }
    });

    let shell = match (flat, taken) {
        (true, true) => "bg-stone-100",
        (true, false) => "",
        (false, true) => "rounded-2xl mb-2 bg-stone-100",
        (false, false) => "bg-[#fffdfb] rounded-2xl shadow-sm mb-2",
    };
    let edge = if mine && progress == Some(Progress::Working) {
        "border-l-4 border-accent-700"
    } else {
        ""
    };
    let motion = match leaving() {
        0 => "max-h-40",
        1 => "max-h-40 opacity-0 translate-x-8",
        _ => "max-h-0 !mb-0 opacity-0 translate-x-8",
    };

    rsx! {
        li {
            class: "overflow-hidden motion-safe:transition-all motion-safe:duration-300 {shell} {edge} {motion}",
            aria_disabled: if taken { "true" } else { "false" },
            div { class: "flex items-center gap-1 pl-1 pr-3 min-h-16",
                if taken {
                    span { class: "size-12 shrink-0 grid place-items-center",
                        match progress {
                            Some(Progress::Working) => rsx! { span { class: "size-6 rounded-full grid place-items-center bg-stone-300 text-stone-700", Icon { name: "play_arrow", class: "!text-[16px]" } } },
                            Some(Progress::Paused) => rsx! { span { class: "size-6 rounded-full grid place-items-center border-2 border-stone-300 text-stone-600", Icon { name: "pause", class: "!text-[16px]" } } },
                            _ => rsx! { span { class: "size-6 rounded-full grid place-items-center border-2 border-dashed border-stone-400 text-stone-600", Icon { name: "flag", class: "!text-[16px]" } } },
                        }
                    }
                } else {
                    label { class: "relative size-12 shrink-0 grid place-items-center cursor-pointer",
                        input {
                            r#type: "checkbox",
                            // Transparent over the circle: a real click lands on the real checkbox.
                            class: "peer absolute inset-0 size-full opacity-0 cursor-pointer",
                            aria_label: "Done: {task.title}",
                            checked: leaving() > 0,
                            onchange: move |_| {
                                spawn(async move {
                                    leaving.set(1);
                                    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                                    leaving.set(2);
                                    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                                    state.complete_task(id);
                                });
                            },
                        }
                        span { class: "size-6 rounded-full border-2 border-stone-400 grid place-items-center transition-colors hover:border-accent-700 peer-checked:bg-accent-700 peer-checked:border-accent-700 motion-safe:peer-checked:animate-pop peer-focus-visible:outline-2 peer-focus-visible:outline-offset-2 peer-focus-visible:outline-accent-700",
                            Icon { name: "check", class: "!text-[16px] text-white" }
                        }
                    }
                }
                button {
                    class: "flex-1 min-w-0 text-left py-3 disabled:cursor-default",
                    disabled: taken,
                    onclick: move |_| nav.write().detail = Some(id),
                    span { class: if taken { "block text-base truncate text-stone-600" } else { "block text-base truncate" }, "{task.title}" }
                    if let Some(words) = state_words {
                        span { class: "sr-only", "{words}" }
                    }
                }
                match holder {
                    Some(who) => rsx! {
                        span { class: if taken { "shrink-0 pl-2 grayscale opacity-60" } else { "shrink-0 pl-2" },
                            Avatar { name: who.name, picture: who.picture, size: Size::Small }
                        }
                    },
                    None => rsx! {
                        button {
                            class: "group shrink-0 size-12 -mr-2 grid place-items-center",
                            aria_label: "Assign {task.title}",
                            onclick: move |_| sheet.set(Some(SheetKind::Assign(id))),
                            span { class: "size-8 rounded-full grid place-items-center bg-stone-100 border-2 border-dashed border-white shadow-[0_1px_3px_rgb(41_37_36/.3)] text-stone-500 transition-colors group-hover:text-accent-700",
                                Icon { name: "person_add", class: "!text-[18px]" }
                            }
                        }
                    },
                }
            }
        }
    }
}
