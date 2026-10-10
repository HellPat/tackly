//! The Tasks tab: what is open, what is done, and tasks that two people
//! finished.

use chrono::{DateTime, Utc};
use dioxus::prelude::*;
use tackly_protocol::{CompletionClaim, Task, TaskStatus};
use uuid::Uuid;

use super::format::{ago, duration};
use crate::state::AppState;

#[component]
pub fn TasksTab(open_finish: EventHandler<Uuid>) -> Element {
    let state = use_context::<AppState>();
    let family = (state.family)();
    if family.tasks.is_empty() {
        return rsx! {
            div { class: "empty",
                div { class: "big", "🌱" }
                p { "No tasks yet. Add the first one." }
            }
        };
    }

    let conflicts: Vec<&Task> = family
        .tasks
        .values()
        .filter(|task| task.has_conflict())
        .collect();
    let mut todo: Vec<&Task> = family
        .tasks
        .values()
        .filter(|task| !task.is_done())
        .collect();
    // What someone is working on comes first.
    todo.sort_by_key(|task| !matches!(task.status, TaskStatus::InProgress { .. }));
    let mut done: Vec<&Task> = family
        .tasks
        .values()
        .filter(|task| task.is_done() && !task.has_conflict())
        .collect();
    // The latest finished comes first.
    done.sort_by_key(|task| match &task.status {
        TaskStatus::Done { at, .. } => std::cmp::Reverse(*at),
        _ => std::cmp::Reverse(DateTime::<Utc>::MIN_UTC),
    });

    rsx! {
        for task in conflicts {
            ConflictCard { key: "{task.id}", task: task.clone() }
        }
        if !todo.is_empty() {
            div { class: "section", "To do" }
        }
        for task in todo {
            TaskCard { key: "{task.id}", task: task.clone(), open_finish }
        }
        if !done.is_empty() {
            div { class: "section", "Done" }
        }
        for task in done {
            TaskCard { key: "{task.id}", task: task.clone(), open_finish }
        }
    }
}

#[component]
fn TaskCard(task: Task, open_finish: EventHandler<Uuid>) -> Element {
    let state = use_context::<AppState>();
    let me = (state.my_id)();
    let id = task.id;
    let class = match task.status {
        TaskStatus::Open => "card",
        TaskStatus::InProgress { .. } => "card doing",
        TaskStatus::Done { .. } => "card done",
    };
    // Starting is offered when nobody (or somebody else) is on it.
    let can_start = match &task.status {
        TaskStatus::Open => true,
        TaskStatus::InProgress { by, .. } => *by != me,
        TaskStatus::Done { .. } => false,
    };
    rsx! {
        div { class: "{class}",
            div { class: "emoji", aria_hidden: "true", "{task.emoji}" }
            div { class: "body",
                div { class: "title", "{task.title}" }
                TaskDetails { status: task.status.clone() }
                div { class: "actions",
                    if can_start {
                        button { class: "btn tonal", onclick: move |_| state.start_task(id), "Start" }
                    }
                    if task.is_done() {
                        button { class: "btn text", onclick: move |_| state.reopen_task(id), "Reopen" }
                    } else {
                        button { class: "btn", onclick: move |_| open_finish.call(id), "Finish" }
                    }
                }
            }
        }
    }
}

/// Who is working on a task, or who finished it and how.
#[component]
fn TaskDetails(status: TaskStatus) -> Element {
    let state = use_context::<AppState>();
    let (family, now, me) = ((state.family)(), (state.now)(), (state.my_id)());
    let name = |device: Uuid| family.member_name(device).to_owned();
    match status {
        TaskStatus::Open => rsx! {},
        TaskStatus::InProgress { by, since, .. } => {
            let who = if by == me {
                "You are".to_owned()
            } else {
                format!("{} is", name(by))
            };
            rsx! {
                div { class: "meta", "{who} on it · started {ago(now, since)}" }
            }
        }
        TaskStatus::Done {
            by, at, metadata, ..
        } => {
            let who = if by == me { "You".to_owned() } else { name(by) };
            let took = metadata.duration_seconds.map(duration);
            let place = metadata
                .location
                .map(|spot| format!("📍 {:.4}, {:.4}", spot.latitude, spot.longitude));
            rsx! {
                div { class: "meta", "Done by {who} · {ago(now, at)}" }
                div { class: "chips",
                    if let Some(took) = took {
                        span { class: "chip ok", "⏱ {took}" }
                    }
                    if let Some(place) = place {
                        span { class: "chip", "{place}" }
                    }
                    if let Some(note) = metadata.note {
                        span { class: "chip", "💬 {note}" }
                    }
                }
            }
        }
    }
}

/// A task that two people finished: whoever took part picks the winner.
#[component]
fn ConflictCard(task: Task) -> Element {
    let state = use_context::<AppState>();
    let (family, me) = ((state.family)(), (state.my_id)());
    let took_part = task.claims.iter().any(|claim| claim.by == me);
    let names = task
        .claims
        .iter()
        .map(|claim| family.member_name(claim.by))
        .collect::<Vec<_>>()
        .join(" and ");
    rsx! {
        div { class: "card conflict",
            div { class: "title", "{task.emoji} {task.title}: finished twice" }
            div { class: "meta", "{names} both finished this task." }
            if took_part {
                div { class: "actions",
                    for claim in task.claims.clone() {
                        KeepButton { key: "{claim.completion_event_id}", task: task.id, claim }
                    }
                }
            } else {
                div { class: "meta", "Waiting for {names} to decide." }
            }
        }
    }
}

#[component]
fn KeepButton(task: Uuid, claim: CompletionClaim) -> Element {
    let state = use_context::<AppState>();
    let name = (state.family)().member_name(claim.by).to_owned();
    rsx! {
        button {
            class: "btn tonal",
            onclick: move |_| state.resolve_conflict(task, claim.completion_event_id),
            "Keep {name}'s"
        }
    }
}
