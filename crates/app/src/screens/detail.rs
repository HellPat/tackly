//! One task in focus, full screen and without the navigation: a big clock for
//! today, the total since it was (re)opened, Start or Pause, and Finish.

use chrono::{Duration, Local, TimeZone, Utc};
use dioxus::prelude::*;
use uuid::Uuid;

use super::home::Nav;
use crate::{
    state::AppState,
    ui::{Icon, PRIMARY, SECONDARY},
};

#[component]
pub fn Detail(task: Uuid) -> Element {
    let state = use_context::<AppState>();
    let mut nav = use_context::<Signal<Nav>>();
    let (family, now) = ((state.family)(), (state.now)());
    let Some(task) = family.tasks.get(&task).cloned() else {
        return rsx! {};
    };
    let id = task.id;
    let running = task.is_running();
    let (total, today) = task.worked(now, start_of_today());
    let places: Vec<String> = task
        .place_ids
        .iter()
        .filter_map(|id| family.places.get(id))
        .map(|place| place.name.clone())
        .collect();
    rsx! {
        section { class: "absolute inset-0 z-40 flex flex-col bg-[#fffdfb]", aria_label: "Task",
            div { class: "flex items-center px-2 h-16 shrink-0",
                button {
                    class: "size-12 grid place-items-center rounded-full text-stone-700 hover:bg-stone-100",
                    aria_label: "Back",
                    onclick: move |_| nav.write().detail = None,
                    Icon { name: "arrow_back" }
                }
            }
            div { class: "px-6 flex flex-col gap-3",
                h1 { class: "text-3xl font-semibold tracking-tight", "{task.title}" }
                if !places.is_empty() {
                    div { class: "flex flex-wrap gap-1.5",
                        for place in places {
                            span { key: "{place}", class: "inline-flex items-center h-6 px-2.5 rounded-full bg-stone-100 text-stone-700 text-[13px] font-semibold", "{place}" }
                        }
                    }
                }
            }
            div { class: "flex-1 grid place-items-center",
                div { class: if running { "size-64 rounded-full grid place-items-center border-[6px] border-accent-700 bg-linear-160 from-accent-50 to-accent-100 motion-safe:animate-breathe" } else { "size-64 rounded-full grid place-items-center border-[6px] border-transparent bg-stone-100" },
                    div { class: "text-center",
                        div { class: "text-xs font-semibold uppercase tracking-wider text-stone-600", "Today" }
                        div { class: "mt-1 text-5xl font-semibold tabular-nums", role: "timer", aria_label: "Worked today", "{clock(today)}" }
                        div { class: "mt-2 text-sm text-stone-600 tabular-nums", "{short(total)} in total" }
                    }
                }
            }
            div { class: "px-6 pb-10 grid grid-cols-2 gap-3",
                if running {
                    button { class: "{SECONDARY} !h-14 text-base", onclick: move |_| state.pause_task(id),
                        Icon { name: "pause" }
                        "Pause"
                    }
                    button { class: "{PRIMARY} !h-14 text-base", onclick: move |_| finish(state, nav, id),
                        Icon { name: "check" }
                        "Finish"
                    }
                } else {
                    button { class: "{PRIMARY} !h-14 text-base", onclick: move |_| state.start_task(id),
                        Icon { name: "play_arrow" }
                        "Start"
                    }
                    button { class: "{SECONDARY} !h-14 text-base", onclick: move |_| finish(state, nav, id),
                        Icon { name: "check" }
                        "Finish"
                    }
                }
            }
        }
    }
}

fn finish(state: AppState, mut nav: Signal<Nav>, task: Uuid) {
    nav.write().detail = None;
    state.complete_task(task);
}

/// Midnight today on this phone's clock.
fn start_of_today() -> chrono::DateTime<Utc> {
    let today = Local::now()
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .unwrap_or_default();
    Local
        .from_local_datetime(&today)
        .earliest()
        .map_or_else(Utc::now, |midnight| midnight.with_timezone(&Utc))
}

/// 1:05:09
fn clock(time: Duration) -> String {
    let seconds = time.num_seconds().max(0);
    format!(
        "{}:{:02}:{:02}",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60
    )
}

/// 12 min, 1 h 5 min
fn short(time: Duration) -> String {
    let minutes = time.num_minutes().max(0);
    if minutes < 60 {
        format!("{minutes} min")
    } else {
        format!("{} h {} min", minutes / 60, minutes % 60)
    }
}
