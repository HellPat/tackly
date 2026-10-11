//! Problems: things that go wrong at home, again and again ("Out of food").
//! The tab lists the open ones; step into one to work on its steps, and edit
//! it from the top bar (its name, who has it, its solutions). "Something’s
//! wrong" notices one for me or raises one for someone else, and suggests
//! the problems the family had before.

use chrono::{DateTime, Utc};
use dioxus::prelude::*;
use tackly_domain::{Problem, Task};
use uuid::Uuid;

use super::{
    home::{Nav, SheetKind, Tab},
    tasks::TaskCard,
};
use crate::{
    state::AppState,
    ui::{Avatar, CARD, EmptyState, FIELD, Heading, Icon, PRIMARY, SECONDARY, Sheet, Size},
};

/// A common problem: its title, icon, and solutions.
type Common = (&'static str, &'static str, &'static [&'static str]);

/// Problems most homes have, ready to notice.
pub const COMMON: [Common; 5] = [
    ("Out of food", "kitchen", &["Cook something"]),
    ("Plants are dry", "potted_plant", &["Water the plants"]),
    (
        "Laundry piles up",
        "local_laundry_service",
        &["Wash a load", "Hang it up", "Put it away"],
    ),
    (
        "Bins are full",
        "delete",
        &["Take out the trash", "Take out the paper"],
    ),
    (
        "Bathroom is dirty",
        "shower",
        &["Clean the sink", "Clean the toilet"],
    ),
];

/// How long it has been open, said plainly.
pub fn open_for(problem: &Problem, now: DateTime<Utc>) -> String {
    let Some(opening) = &problem.opening else {
        return String::new();
    };
    let hours = (now - opening.since).num_hours();
    match hours {
        ..1 => "Open since just now".into(),
        1..24 => format!("Open for {hours} h"),
        24..48 => "Open since yesterday".into(),
        _ => format!("Open for {} days", hours / 24),
    }
}

#[component]
pub fn ProblemIcon(icon: String, open: bool) -> Element {
    rsx! {
        span { class: if open { "size-9 shrink-0 rounded-full grid place-items-center bg-amber-100 text-amber-900" } else { "size-9 shrink-0 rounded-full grid place-items-center bg-stone-100 text-stone-500" },
            Icon { name: icon, class: "!text-[20px]" }
        }
    }
}

#[component]
pub fn ProblemsTab() -> Element {
    let state = use_context::<AppState>();
    let mut nav = use_context::<Signal<Nav>>();
    let (family, now) = ((state.family)(), (state.now)());
    let here = nav();
    if let Some(id) = here.problem {
        return if here.editing_problem {
            rsx! { EditProblem { problem: id } }
        } else {
            rsx! { ProblemView { problem: id } }
        };
    }
    let open: Vec<Problem> = family.open_problems().cloned().collect();
    if open.is_empty() {
        return rsx! { EmptyState { icon: "sentiment_satisfied", title: "No problems", line: "All calm at home." } };
    }
    rsx! {
        ul { aria_label: "Open problems",
            for problem in open {
                li { key: "{problem.id}", class: "{CARD} mb-2",
                    button {
                        class: "w-full flex items-center gap-3 pl-3 pr-3 min-h-16 text-left",
                        onclick: {
                            let id = problem.id;
                            move |_| nav.write().problem = Some(id)
                        },
                        ProblemIcon { icon: problem.icon.clone(), open: true }
                        span { class: "flex-1 min-w-0",
                            span { class: "block truncate", "{problem.title}" }
                            span { class: "block text-sm text-stone-600", "{open_for(&problem, now)}" }
                        }
                        if let Some(holder) = problem.holder().and_then(|id| family.members.get(&id)).cloned() {
                            Avatar { name: holder.name, picture: holder.picture, size: Size::Small }
                        }
                        Icon { name: "chevron_right", class: "text-stone-500" }
                    }
                }
            }
        }
    }
}

/// One problem: its steps, to work on right here, and Resolve.
#[component]
fn ProblemView(problem: Uuid) -> Element {
    let state = use_context::<AppState>();
    let mut sheet = use_context::<Signal<Option<SheetKind>>>();
    let (family, now, me) = ((state.family)(), (state.now)(), (state.my_id)());
    let Some(found) = family.problems.get(&problem).cloned() else {
        return rsx! {};
    };
    if !found.open {
        return rsx! {
            p { class: "py-12 text-center text-stone-600", "Resolved. Notice it again if it’s back." }
            div { class: "grid grid-cols-2 gap-2",
                button { class: "{SECONDARY} !h-12", onclick: move |_| sheet.set(Some(SheetKind::Raise(Some(problem), ""))), "Raise" }
                button {
                    class: "{PRIMARY} !h-12",
                    onclick: move |_| state.notice_problem(Some(problem), found.title.clone(), String::new(), vec![], me),
                    "Notice"
                }
            }
        };
    }
    let steps: Vec<Task> = family.steps(problem).cloned().collect();
    rsx! {
        p { class: "px-2 pb-3 text-sm text-stone-600", "{open_for(&found, now)}" }
        if steps.is_empty() {
            EmptyState { icon: "task_alt", title: "All steps done", line: "Fixed? Then resolve it." }
        }
        ul {
            for task in steps.clone() {
                TaskCard { key: "{task.id}", task }
            }
        }
        div { class: "pt-4",
            button {
                class: if steps.is_empty() { "{PRIMARY} w-full !h-12" } else { "{SECONDARY} w-full !h-12" },
                onclick: move |_| state.resolve_problem(problem),
                Icon { name: "done_all", class: "!text-[20px]" }
                "Resolve"
            }
        }
    }
}

/// Editing a problem: its name, who has it (while open), and its solutions.
#[component]
fn EditProblem(problem: Uuid) -> Element {
    let state = use_context::<AppState>();
    let mut sheet = use_context::<Signal<Option<SheetKind>>>();
    let family = (state.family)();
    let mut typed = use_signal(String::new);
    let mut new_solution = use_signal(String::new);
    let Some(found) = family.problems.get(&problem).cloned() else {
        return rsx! {};
    };
    let holder = found
        .holder()
        .and_then(|id| family.members.get(&id))
        .cloned();
    let me = (state.my_id)();
    let save = move |title: String| {
        let current = state
            .family
            .peek()
            .problems
            .get(&problem)
            .map(|p| p.title.clone());
        if !title.trim().is_empty() && current.as_deref() != Some(title.trim()) {
            state.rename_problem(problem, title);
        }
    };
    rsx! {
        Heading { text: "Name" }
        input {
            class: FIELD,
            aria_label: "Problem name",
            value: "{found.title}",
            onchange: move |event| save(event.value()),
            oninput: move |event| typed.set(event.value()),
            onkeydown: move |event| {
                if event.key() == Key::Enter {
                    save(typed());
                }
            },
        }
        if let Some(holder) = holder {
            Heading { text: "Who has it" }
            div { class: "{CARD} flex items-center gap-3 pl-4 pr-2 min-h-16",
                span { class: "flex-1", if holder.device_id == me { "Me" } else { "{holder.name}" } }
                button {
                    class: "size-11 grid place-items-center",
                    aria_label: "Assign {found.title}",
                    onclick: move |_| sheet.set(Some(SheetKind::AssignProblem(problem))),
                    Avatar { name: holder.name.clone(), picture: holder.picture.clone(), size: Size::Small }
                }
            }
        }
        Heading { text: "Solutions" }
        ul { class: "{CARD} overflow-hidden divide-y divide-stone-100", aria_label: "Solutions", "data-testid": "solutions",
            for solution in found.solutions.clone() {
                li { key: "{solution.id}", class: "flex items-center gap-1 pl-1 pr-2 min-h-14",
                    span { class: "size-11 shrink-0 grid place-items-center text-stone-400",
                        Icon { name: "task_alt", class: "!text-[20px]" }
                    }
                    span { class: "flex-1 min-w-0",
                        span { class: "block truncate", "{solution.title}" }
                    }
                    button {
                        class: "size-10 grid place-items-center rounded-full text-stone-500 hover:bg-stone-100",
                        aria_label: "Remove {solution.title}",
                        onclick: {
                            let id = solution.id;
                            move |_| state.remove_solution(problem, id)
                        },
                        Icon { name: "close", class: "!text-[20px]" }
                    }
                }
            }
            li { class: "flex items-center gap-1 pl-1 pr-3 min-h-14",
                span { class: "size-11 grid place-items-center text-accent-700", Icon { name: "add", class: "!text-[20px]" } }
                input {
                    class: "flex-1 min-w-0 h-12 bg-transparent outline-none placeholder:text-stone-500",
                    aria_label: "Add a solution",
                    placeholder: "Add a solution",
                    value: "{new_solution}",
                    oninput: move |event| new_solution.set(event.value()),
                    onkeydown: move |event| {
                        if event.key() == Key::Enter && !new_solution().trim().is_empty() {
                            state.add_solution(problem, new_solution());
                            new_solution.set(String::new());
                        }
                    },
                }
            }
        }
        p { class: "px-2 pt-2 text-sm text-stone-600", "When it’s noticed, every solution becomes a task for whoever has the problem." }
        if !found.open {
            div { class: "pt-5 grid grid-cols-2 gap-2",
                button { class: "{SECONDARY} !h-12", onclick: move |_| sheet.set(Some(SheetKind::Raise(Some(problem), ""))), "Raise" }
                button {
                    class: "{PRIMARY} !h-12",
                    onclick: {
                        let title = found.title.clone();
                        move |_| state.notice_problem(Some(problem), title.clone(), String::new(), vec![], me)
                    },
                    "Notice"
                }
            }
        }
    }
}

/// A problem to notice: one the family knows, or a common one not made yet.
#[derive(Clone, PartialEq)]
struct Suggestion {
    known: Option<Uuid>,
    title: String,
    template: &'static str,
    icon: String,
    solutions: Vec<String>,
}

/// What's wrong? Type it: problems the family had before (and common ones)
/// are suggested, to bring back with Notice or Raise; or make a new one.
#[component]
pub fn SomethingWrongSheet() -> Element {
    let state = use_context::<AppState>();
    let mut sheet = use_context::<Signal<Option<SheetKind>>>();
    let mut nav = use_context::<Signal<Nav>>();
    let (family, me) = ((state.family)(), (state.my_id)());
    let mut typed = use_signal(String::new);
    let wanted = typed().trim().to_lowercase();
    let mut suggestions: Vec<Suggestion> = family
        .problems
        .values()
        .filter(|problem| !problem.open)
        .map(|problem| Suggestion {
            known: Some(problem.id),
            title: problem.title.clone(),
            template: "",
            icon: problem.icon.clone(),
            solutions: problem.solutions.iter().map(|s| s.title.clone()).collect(),
        })
        .collect();
    for (title, icon, solutions) in COMMON {
        if !family
            .problems
            .values()
            .any(|problem| problem.title == title)
        {
            suggestions.push(Suggestion {
                known: None,
                title: title.into(),
                template: title,
                icon: icon.into(),
                solutions: solutions.iter().map(|t| (*t).to_owned()).collect(),
            });
        }
    }
    suggestions.retain(|s| wanted.is_empty() || s.title.to_lowercase().contains(&wanted));
    let exists = family
        .problems
        .values()
        .any(|problem| problem.title.to_lowercase() == wanted);
    rsx! {
        Sheet { close: move |_| sheet.set(None), label: "Something’s wrong",
            h2 { class: "text-xl font-semibold pb-3", "What’s wrong?" }
            input {
                class: FIELD,
                aria_label: "What’s wrong?",
                placeholder: "e.g. Out of food",
                autofocus: true,
                value: "{typed}",
                oninput: move |event| typed.set(event.value()),
            }
            ul { class: "pt-2 max-h-[50dvh] overflow-y-auto", aria_label: "Suggestions",
                if !suggestions.is_empty() {
                    li { class: "pt-2 pb-1 text-sm font-semibold text-stone-600",
                        if wanted.is_empty() { "Problems you know" } else { "Had this before?" }
                    }
                }
                for suggestion in suggestions {
                    li { key: "{suggestion.title}", class: "flex items-center gap-2 min-h-14 border-b border-stone-100",
                        ProblemIcon { icon: suggestion.icon.clone(), open: false }
                        span { class: "flex-1 min-w-0",
                            span { class: "block truncate", "{suggestion.title}" }
                            span { class: "block text-xs text-stone-600 truncate",
                                if suggestion.solutions.is_empty() { "No solutions yet" } else { {suggestion.solutions.join(" · ")} }
                            }
                        }
                        button {
                            class: "{SECONDARY} !h-9 !px-3 text-sm",
                            aria_label: "Raise {suggestion.title}",
                            onclick: {
                                let (known, template) = (suggestion.known, suggestion.template);
                                move |_| sheet.set(Some(SheetKind::Raise(known, template)))
                            },
                            "Raise"
                        }
                        button {
                            class: "{PRIMARY} !h-9 !px-3 text-sm",
                            aria_label: "Notice {suggestion.title}",
                            onclick: {
                                let suggestion = suggestion.clone();
                                move |_| {
                                    sheet.set(None);
                                    let s = suggestion.clone();
                                    state.notice_problem(s.known, s.title, s.icon, s.solutions, me);
                                }
                            },
                            "Notice"
                        }
                    }
                }
                if !wanted.is_empty() && !exists {
                    li {
                        button {
                            class: "w-full flex items-center gap-3 min-h-14 text-left",
                            onclick: move |_| {
                                sheet.set(None);
                                state.create_problem(typed().trim().to_owned(), move |id| {
                                    nav.set(Nav { tab: Tab::Problems, problem: Some(id), editing_problem: true, ..Nav::default() });
                                });
                            },
                            span { class: "size-9 shrink-0 rounded-full grid place-items-center bg-accent-100 text-accent-800", Icon { name: "add", class: "!text-[20px]" } }
                            span { class: "flex-1", "New problem “{typed().trim()}”" }
                            Icon { name: "chevron_right", class: "text-stone-500" }
                        }
                    }
                }
            }
        }
    }
}

/// Raise a problem for someone: everyone but me.
#[component]
pub fn RaiseSheet(problem: Option<Uuid>, title: &'static str) -> Element {
    let state = use_context::<AppState>();
    let mut sheet = use_context::<Signal<Option<SheetKind>>>();
    let (family, me) = ((state.family)(), (state.my_id)());
    let (name, icon, solutions) = match problem.and_then(|id| family.problems.get(&id)) {
        Some(known) => (known.title.clone(), known.icon.clone(), vec![]),
        None => COMMON
            .iter()
            .find(|(common, _, _)| *common == title)
            .map(|(common, icon, solutions)| {
                (
                    (*common).to_owned(),
                    (*icon).to_owned(),
                    solutions.iter().map(|t| (*t).to_owned()).collect(),
                )
            })
            .unwrap_or_default(),
    };
    rsx! {
        Sheet { close: move |_| sheet.set(None), label: "Raise {name} for",
            h2 { class: "text-xl font-semibold", "Raise “{name}” for" }
            ul { class: "mt-4 -mx-2",
                for member in family.members.values().filter(|member| member.device_id != me).cloned() {
                    li { key: "{member.device_id}",
                        button {
                            class: "w-full flex items-center gap-4 px-2 min-h-14 rounded-lg text-left hover:bg-stone-100",
                            onclick: {
                                let (who, name, icon, solutions) = (member.device_id, name.clone(), icon.clone(), solutions.clone());
                                move |_| {
                                    sheet.set(None);
                                    state.notice_problem(problem, name.clone(), icon.clone(), solutions.clone(), who);
                                }
                            },
                            Avatar { name: member.name.clone(), picture: member.picture.clone(), size: Size::Medium }
                            span { class: "text-base", "{member.name}" }
                        }
                    }
                }
            }
        }
    }
}

/// Who has a problem from now on (with its open steps): everyone, me first.
#[component]
pub fn AssignProblemSheet(problem: Uuid) -> Element {
    let state = use_context::<AppState>();
    let mut sheet = use_context::<Signal<Option<SheetKind>>>();
    let (family, me) = ((state.family)(), (state.my_id)());
    let title = family
        .problems
        .get(&problem)
        .map(|p| p.title.clone())
        .unwrap_or_default();
    let mut people: Vec<_> = family.members.values().cloned().collect();
    people.sort_by_key(|member| member.device_id != me);
    rsx! {
        Sheet { close: move |_| sheet.set(None), label: "Who has {title}?",
            h2 { class: "text-xl font-semibold", "Who has “{title}”?" }
            ul { class: "mt-4 -mx-2",
                for member in people {
                    li { key: "{member.device_id}",
                        button {
                            class: "w-full flex items-center gap-4 px-2 min-h-14 rounded-lg text-left hover:bg-stone-100",
                            onclick: {
                                let who = member.device_id;
                                move |_| {
                                    sheet.set(None);
                                    state.assign_problem(problem, who);
                                }
                            },
                            Avatar { name: member.name.clone(), picture: member.picture.clone(), size: Size::Medium }
                            span { class: "text-base", if member.device_id == me { "Me" } else { "{member.name}" } }
                        }
                    }
                }
            }
        }
    }
}
