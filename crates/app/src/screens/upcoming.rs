//! Upcoming: planning the next day. It opens by itself at my planning time,
//! covers everything (no navigation), and has two ways out: Plan later, or
//! Save plan.
//!
//! On top, the focus (the free slots). Below, my open problems (step into one
//! to see its steps) and what is left over. Anything below moves up with its
//! arrow or by dragging; in the focus it moves back down the same way, and is
//! reordered by dragging. Swiping a row to the right hides it for this round.

use std::collections::HashSet;

use chrono::Duration;
use dioxus::prelude::*;
use tackly_protocol::{Family, Task};
use uuid::Uuid;

use super::{
    gestures::{Gesture, use_gestures},
    problems::{ProblemIcon, open_for},
    today::{place_at, problem_of, reference},
};
use crate::{
    platform,
    settings::SLOTS,
    state::{AppState, Undo},
    theme,
    ui::{CARD, Icon, PRIMARY, SECONDARY},
};

/// Whether Upcoming is up now: my planning time has come, the next day is not
/// planned yet, and planning was not put off.
pub fn should_plan(state: AppState) -> bool {
    let _ = (state.now)(); // re-checked as the clock ticks
    let Some(time) = (state.settings)().plan_time() else {
        return false;
    };
    let now = platform::local_now();
    let tomorrow = now.date_naive() + Duration::days(1);
    let snoozed = (state.plan_snoozed_until)().is_some_and(|until| now < until);
    now.time() >= time && !snoozed && (state.family)().plan((state.my_id)(), tomorrow).is_none()
}

/// The open tasks that may be planned (mine or free).
fn plannable<'a>(family: &'a Family, me: Uuid, ids: &'a [Uuid]) -> impl Iterator<Item = &'a Task> {
    ids.iter()
        .filter_map(|id| family.tasks.get(id))
        .filter(move |task| !task.is_done() && task.assignee.is_none_or(|who| who == me))
}

#[component]
pub fn Upcoming() -> Element {
    let state = use_context::<AppState>();
    let mut upcoming = state.upcoming;
    let (family, me) = ((state.family)(), (state.my_id)());
    let today = platform::today();
    let tomorrow = today + Duration::days(1);
    // A fresh round: start from tomorrow's plan, if there is one.
    use_hook(move || {
        if upcoming.peek().day != Some(tomorrow) {
            let focus = state
                .family
                .peek()
                .plan(me, tomorrow)
                .map(<[Uuid]>::to_vec)
                .unwrap_or_default();
            upcoming.set(crate::state::Upcoming {
                focus,
                day: Some(tomorrow),
                ..Default::default()
            });
        }
    });
    let draft = upcoming();
    let scheme = theme::scheme(&(state.settings)().scheme);
    let on_gesture = use_callback(move |gesture: Gesture| match gesture {
        Gesture::Drop {
            r#ref,
            to,
            index,
            from,
        } => match (reference(&r#ref), to.as_str(), from.as_str()) {
            (Some(('t', task)), "focus", "focus") => {
                place_at(&mut upcoming.write().focus, task, index)
            }
            (Some(('t', task)), "focus", _) => into_focus(state, task, Some(index)),
            (Some(('t', task)), "pool", "focus") => upcoming.write().focus.retain(|id| *id != task),
            _ => {}
        },
        Gesture::Swipe { r#ref } => {
            let title = row_title(&state.family.peek(), &r#ref);
            upcoming.write().dismissed.push(r#ref.clone());
            state.toast(
                format!("{title}: not this time"),
                Some(Undo::Undismiss(r#ref)),
            );
        }
    });
    use_gestures("upcoming", on_gesture);

    let focus: Vec<Task> = plannable(&family, me, &draft.focus).cloned().collect();
    let full = focus.len() >= SLOTS;
    let in_focus: HashSet<Uuid> = draft.focus.iter().copied().collect();
    let hidden = |reference: String| draft.dismissed.contains(&reference);
    let back_to_list = draft.problem.is_some();

    let pool = match draft
        .problem
        .and_then(|id| family.problems.get(&id))
        .cloned()
    {
        // Stepped into a problem: its steps, in no particular order.
        Some(problem) => {
            let steps: Vec<Task> = family
                .steps(problem.id)
                .filter(|task| !in_focus.contains(&task.id) && !hidden(format!("t:{}", task.id)))
                .filter(|task| task.assignee.is_none_or(|who| who == me))
                .cloned()
                .collect();
            rsx! {
                div { class: "pt-5 pb-2 flex items-center gap-3",
                    button { class: "{SECONDARY} !h-11 !pl-3", onclick: move |_| upcoming.write().problem = None,
                        Icon { name: "arrow_back", class: "!text-[20px]" }
                        "All problems"
                    }
                    h2 { class: "flex-1 min-w-0 truncate text-base font-semibold", "{problem.title}" }
                }
                if steps.is_empty() {
                    p { class: "py-8 text-center text-stone-600", "No steps left here." }
                }
                ul {
                    for task in steps {
                        PlanRow { key: "{task.id}", task, sub: String::new(), in_focus: false, full }
                    }
                }
            }
        }
        None => {
            let problems: Vec<_> = family
                .open_problems()
                .filter(|problem| {
                    problem.holder() == Some(me) && !hidden(format!("p:{}", problem.id))
                })
                .cloned()
                .collect();
            let from_today: HashSet<Uuid> =
                family.focus(me, today).iter().map(|task| task.id).collect();
            let mut leftovers: Vec<Task> = family
                .open_tasks()
                .filter(|task| task.assignee == Some(me) && task.problem_id.is_none())
                .filter(|task| !in_focus.contains(&task.id) && !hidden(format!("t:{}", task.id)))
                .cloned()
                .collect();
            leftovers.sort_by_key(|task| !from_today.contains(&task.id));
            let now = (state.now)();
            let nothing = problems.is_empty() && leftovers.is_empty();
            rsx! {
                if !problems.is_empty() {
                    h2 { class: "px-2 pt-5 pb-2 text-sm font-semibold text-stone-600", "Problems" }
                    ul {
                        for problem in problems {
                            li {
                                key: "{problem.id}",
                                class: "{CARD} mb-2 touch-pan-y",
                                "data-ref": "p:{problem.id}",
                                "data-testid": "problem",
                                "data-swipe": "true",
                                button {
                                    class: "w-full flex items-center gap-3 pl-3 pr-3 min-h-16 text-left",
                                    onclick: {
                                        let id = problem.id;
                                        move |_| upcoming.write().problem = Some(id)
                                    },
                                    ProblemIcon { icon: problem.icon.clone(), open: true }
                                    span { class: "flex-1 min-w-0",
                                        span { class: "block truncate", "{problem.title}" }
                                        span { class: "block text-sm text-stone-600", "{open_for(&problem, now)}" }
                                    }
                                    Icon { name: "chevron_right", class: "text-stone-500" }
                                }
                            }
                        }
                    }
                }
                if !leftovers.is_empty() {
                    h2 { class: "px-2 pt-5 pb-2 text-sm font-semibold text-stone-600", "Left over" }
                    ul {
                        for task in leftovers {
                            PlanRow {
                                key: "{task.id}",
                                sub: if from_today.contains(&task.id) { "From today".to_owned() } else { "Yours".to_owned() },
                                task,
                                in_focus: false,
                                full,
                            }
                        }
                    }
                }
                if nothing {
                    p { class: "py-8 text-center text-stone-600", "Nothing left over. A fresh start." }
                }
            }
        }
    };

    rsx! {
        section {
            class: "absolute inset-0 z-40 flex flex-col overflow-hidden text-stone-800 {scheme.sky}",
            style: "{scheme.accent_vars()}",
            aria_label: "Upcoming",
            "data-scope": "upcoming",
            "data-testid": "upcoming",
            div { class: "absolute inset-x-0 bottom-0 h-[42%] pointer-events-none", aria_hidden: "true", dangerous_inner_html: "{scheme.scene_svg()}" }
            header { class: "relative z-10 shrink-0 flex items-center gap-1 px-2 h-16",
                if back_to_list {
                    button {
                        class: "size-12 grid place-items-center rounded-full text-stone-700 hover:bg-stone-900/5",
                        aria_label: "Back",
                        onclick: move |_| upcoming.write().problem = None,
                        Icon { name: "arrow_back" }
                    }
                }
                h1 { class: "flex-1 px-2 text-2xl font-semibold tracking-tight", "Upcoming" }
                span { class: "mr-3 text-sm text-stone-600", "{tomorrow.format(\"%a %-d %b\")}" }
            }
            main { class: "relative z-10 flex-1 overflow-y-auto px-3 pb-6",
                h2 { class: "px-2 pt-2 pb-2 text-sm font-semibold text-stone-600",
                    "Focus "
                    span { class: "font-normal text-stone-500", "{focus.len()}/{SLOTS}" }
                }
                ul { "data-zone": "focus", aria_label: "Focus", "data-testid": "focus",
                    for task in focus.clone() {
                        PlanRow { key: "{task.id}", sub: problem_of(&family, &task), task, in_focus: true, full }
                    }
                    for i in focus.len()..SLOTS {
                        li { key: "free-{i}", class: "mb-2 flex items-center gap-3 pl-4 pr-3 min-h-16 rounded-2xl border-2 border-dashed border-stone-400/70 bg-white/40 text-stone-600",
                            span { class: "size-6 rounded-full border-2 border-dashed border-stone-400" }
                            span { class: "flex-1", "Free slot" }
                        }
                    }
                }
                div { "data-zone": "pool", "data-testid": "pool", {pool} }
            }
            // The only ways out. Stacked: the main action at the bottom.
            section { class: "relative z-10 shrink-0 px-3 pb-3 pt-2 flex flex-col gap-2", aria_label: "Plan",
                button {
                    class: "{SECONDARY} w-full !h-12",
                    onclick: move |_| {
                        let mut snoozed = state.plan_snoozed_until;
                        snoozed.set(Some(platform::local_now() + Duration::minutes(30)));
                        state.say("I’ll ask again in 30 minutes");
                    },
                    "Plan later"
                }
                button {
                    class: "{PRIMARY} w-full !h-12",
                    onclick: move |_| {
                        let me = *state.my_id.peek();
                        let tasks: Vec<Uuid> = plannable(&state.family.peek(), me, &upcoming.peek().focus).map(|task| task.id).collect();
                        state.plan_day(tomorrow, tasks, Some("Planned. Sleep well.".into()));
                    },
                    "Save plan"
                }
            }
        }
    }
}

/// Puts a task into the focus (at `index`, or last), if there is room.
fn into_focus(state: AppState, task: Uuid, index: Option<usize>) {
    let mut upcoming = state.upcoming;
    let me = *state.my_id.peek();
    let room = plannable(&state.family.peek(), me, &upcoming.peek().focus).count() < SLOTS;
    if !room {
        state.say("The focus is full. Move one back first.");
        return;
    }
    let mut draft = upcoming.write();
    let at = index.unwrap_or(draft.focus.len());
    place_at(&mut draft.focus, task, at);
}

/// What a row is called, for the toast after swiping it away.
fn row_title(family: &Family, reference_text: &str) -> String {
    match reference(reference_text) {
        Some(('t', id)) => family.tasks.get(&id).map(|task| task.title.clone()),
        Some((_, id)) => family
            .problems
            .get(&id)
            .map(|problem| problem.title.clone()),
        None => None,
    }
    .unwrap_or_else(|| "It".into())
}

/// A task in Upcoming: a handle, its title, and an arrow up (into the focus) or down (back).
#[component]
fn PlanRow(task: Task, sub: String, in_focus: bool, full: bool) -> Element {
    let state = use_context::<AppState>();
    let mut upcoming = state.upcoming;
    let id = task.id;
    rsx! {
        li {
            class: if in_focus { "{CARD} mb-2 flex items-center gap-2 pl-1 pr-1 min-h-16" } else { "{CARD} mb-2 flex items-center gap-2 pl-1 pr-1 min-h-16 touch-pan-y" },
            "data-ref": "t:{id}",
            "data-testid": "task",
            "data-swipe": (!in_focus).then_some("true"),
            span { class: "size-11 shrink-0 grid place-items-center text-stone-400 touch-none cursor-grab", "data-drag": "true", "data-testid": "drag-handle", aria_hidden: "true",
                Icon { name: "drag_indicator" }
            }
            span { class: "size-9 shrink-0 rounded-full grid place-items-center bg-stone-100 text-stone-600",
                Icon { name: "task_alt", class: "!text-[20px]" }
            }
            span { class: "flex-1 min-w-0",
                span { class: "block truncate", "{task.title}" }
                if !sub.is_empty() {
                    span { class: "block text-sm text-stone-600 truncate", "{sub}" }
                }
            }
            if in_focus {
                button {
                    class: "size-11 shrink-0 grid place-items-center rounded-full text-stone-600 hover:bg-stone-100",
                    aria_label: "Move {task.title} back",
                    onclick: move |_| upcoming.write().focus.retain(|known| *known != id),
                    Icon { name: "arrow_downward" }
                }
            } else {
                button {
                    class: if full { "size-11 shrink-0 grid place-items-center rounded-full text-stone-300" } else { "size-11 shrink-0 grid place-items-center rounded-full text-accent-700 hover:bg-accent-50" },
                    aria_label: "Move {task.title} into focus",
                    onclick: move |_| into_focus(state, id, None),
                    Icon { name: "arrow_upward" }
                }
            }
        }
    }
}
