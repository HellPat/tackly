//! Today: my priorities (up to three, in my order), free slots to pick more
//! into, and what I got done today. When the last priority is done, a kitten
//! celebrates.

use dioxus::prelude::*;
use tackly_protocol::{Family, Task};
use uuid::Uuid;

use super::{
    gestures::{Gesture, use_gestures},
    home::{Nav, Tab},
    tasks::TaskCard,
};
use crate::{
    platform,
    settings::SLOTS,
    state::AppState,
    ui::{CARD, FILLED, Icon, PRIMARY, SECONDARY},
};

#[component]
pub fn TodayTab() -> Element {
    let nav = use_context::<Signal<Nav>>();
    if nav().pick_more {
        rsx! { PickMore {} }
    } else {
        rsx! { Focus {} }
    }
}

/// The problem a task is a step of, by name.
pub fn problem_of(family: &Family, task: &Task) -> String {
    task.problem_id
        .and_then(|id| family.problems.get(&id))
        .map(|problem| problem.title.clone())
        .unwrap_or_default()
}

/// Moves `id` to `index` within `ids`.
pub fn place_at(ids: &mut Vec<Uuid>, id: Uuid, index: usize) {
    ids.retain(|known| *known != id);
    ids.insert(index.min(ids.len()), id);
}

/// A row's reference ("t:<task>" or "p:<problem>") as its kind and ID.
pub fn reference(text: &str) -> Option<(char, Uuid)> {
    let (kind, id) = text.split_once(':')?;
    Some((kind.chars().next()?, id.parse().ok()?))
}

#[component]
fn Focus() -> Element {
    let state = use_context::<AppState>();
    let mut nav = use_context::<Signal<Nav>>();
    let (family, me, _) = ((state.family)(), (state.my_id)(), (state.now)());
    let today = platform::today();
    let focus: Vec<Task> = family.focus(me, today).into_iter().cloned().collect();
    let n = focus.len();
    // Dragged into another order: that order is today's plan from now on.
    let on_gesture = use_callback(move |gesture: Gesture| {
        let Gesture::Drop {
            r#ref, to, index, ..
        } = gesture
        else {
            return;
        };
        let (Some(('t', task)), "focus") = (reference(&r#ref), to.as_str()) else {
            return;
        };
        let today = platform::today();
        let me = *state.my_id.peek();
        let mut ids: Vec<Uuid> = state
            .family
            .peek()
            .focus(me, today)
            .iter()
            .map(|task| task.id)
            .collect();
        place_at(&mut ids, task, index);
        state.plan_day(today, ids, None);
    });
    use_gestures("today", on_gesture);
    rsx! {
        div { "data-scope": "today",
            h2 { class: "px-2 pt-2 pb-2 text-sm font-semibold text-stone-600",
                "Priorities "
                span { class: "font-normal text-stone-500", "{n}/{SLOTS}" }
            }
            ul { "data-zone": "focus", aria_label: "Priorities", "data-testid": "priorities",
                for task in focus {
                    TaskCard { key: "{task.id}", sub: problem_of(&family, &task), handle: n > 1, task }
                }
                for i in n..SLOTS {
                    li { key: "free-{i}", class: "mb-2",
                        button {
                            class: "w-full flex items-center gap-3 pl-4 pr-3 min-h-16 rounded-2xl border-2 border-dashed border-stone-400/70 bg-white/40 text-stone-600 hover:bg-white/70",
                            onclick: move |_| nav.write().pick_more = true,
                            span { class: "size-6 rounded-full border-2 border-dashed border-stone-400" }
                            span { class: "flex-1 text-left", "Free slot" }
                            span { class: "text-sm", "Pick something" }
                            Icon { name: "chevron_right", class: "text-stone-500" }
                        }
                    }
                }
            }
            DonePanel {}
        }
    }
}

const PRAISE: [&str; 4] = [
    "That’s real progress.",
    "Look at you go.",
    "Every one counts.",
    "Done is done. Well done.",
];

/// What I finished today, as an accomplishment. Nothing yet: no panel, no judgement.
#[component]
fn DonePanel() -> Element {
    let state = use_context::<AppState>();
    let (family, me, _) = ((state.family)(), (state.my_id)(), (state.now)());
    let today = platform::today();
    let done: Vec<String> = family
        .tasks
        .values()
        .filter(|task| {
            task.done
                .as_ref()
                .is_some_and(|done| done.by == me && platform::local(done.at).date_naive() == today)
        })
        .map(|task| task.title.clone())
        .collect();
    if done.is_empty() {
        return rsx! {};
    }
    let praise = PRAISE[(done.len() - 1) % PRAISE.len()];
    rsx! {
        section { class: "mt-5 rounded-2xl bg-linear-to-br from-accent-100 to-[#fffdfb] shadow-sm p-5 motion-safe:animate-rise", aria_label: "Done today",
            div { class: "flex items-center gap-4",
                span { class: "size-14 shrink-0 rounded-full grid place-items-center bg-accent-700 text-white shadow-md",
                    Icon { name: "emoji_events", class: "!text-[30px] {FILLED}" }
                }
                span {
                    span { class: "block text-2xl font-semibold tracking-tight", "{done.len()} done today" }
                    span { class: "block text-stone-700", "{praise}" }
                }
            }
            ul { class: "mt-4 flex flex-col gap-1.5",
                for title in done {
                    li { key: "{title}", class: "flex items-center gap-2 text-stone-700",
                        Icon { name: "check_circle", class: "!text-[20px] text-accent-700 {FILLED}" }
                        span { class: "truncate", "{title}" }
                    }
                }
            }
        }
    }
}

/// Free tasks to fill free slots with. Each one taken fills a slot; full, it is back to Today.
#[component]
fn PickMore() -> Element {
    let state = use_context::<AppState>();
    let mut nav = use_context::<Signal<Nav>>();
    let (family, me) = ((state.family)(), (state.my_id)());
    let today = platform::today();
    let focus: Vec<Uuid> = family.focus(me, today).iter().map(|task| task.id).collect();
    let free = SLOTS.saturating_sub(focus.len());
    let tasks: Vec<Task> = family
        .open_tasks()
        .filter(|task| task.assignee.is_none() && task.problem_id.is_none())
        .cloned()
        .collect();
    rsx! {
        div { class: "flex items-center justify-center gap-2 py-3", aria_label: "{free} free",
            for i in 0..SLOTS {
                span { key: "{i}", class: if i < SLOTS - free { "size-3 rounded-full bg-accent-700" } else { "size-3 rounded-full border-2 border-stone-400" } }
            }
        }
        p { class: "pb-3 text-center text-stone-700",
            if free == 1 { "One free slot. Pick what fits." } else { "{free} free slots. Pick what fits." }
        }
        if tasks.is_empty() {
            p { class: "py-16 text-center text-stone-600", "Nothing unassigned. Rest." }
        }
        ul {
            for task in tasks {
                li { key: "{task.id}", class: "{CARD} mb-2 flex items-center gap-3 pl-4 pr-3 min-h-16",
                    span { class: "flex-1 min-w-0 truncate", "{task.title}" }
                    button {
                        class: "{SECONDARY} !h-9 !px-3",
                        aria_label: "Take {task.title}",
                        onclick: {
                            let (id, mut ids) = (task.id, focus.clone());
                            move |_| {
                                ids.push(id);
                                let full = ids.len() >= SLOTS;
                                state.plan_day(today, ids.clone(), full.then(|| "Today is full again".to_owned()));
                                if full {
                                    nav.write().pick_more = false;
                                }
                            }
                        },
                        Icon { name: "add", class: "!text-[18px]" }
                        "Take"
                    }
                }
            }
        }
    }
}

/// All of today's priorities are done: confetti and a happy kitten.
#[component]
pub fn Celebration(kitten: usize) -> Element {
    let state = use_context::<AppState>();
    let mut nav = use_context::<Signal<Nav>>();
    let mut celebrate = state.celebrate;
    let (family, me) = ((state.family)(), (state.my_id)());
    let planned = family.plan(me, platform::today()).map_or(0, <[Uuid]>::len);
    let (svg, name) = kitten_svg(kitten);
    const COLORS: [&str; 6] = [
        "#f472b6", "#facc15", "#38bdf8", "#4ade80", "#a78bfa", "#fb923c",
    ];
    rsx! {
        section { class: "absolute inset-0 z-50 flex flex-col bg-accent-50", aria_label: "Celebration", "data-testid": "celebration",
            div { class: "absolute inset-0 overflow-hidden pointer-events-none motion-reduce:hidden", aria_hidden: "true",
                for i in 0..36_usize {
                    span {
                        key: "{i}",
                        class: "absolute top-0 w-2 h-3 rounded-sm motion-safe:animate-fall",
                        style: "left:{(i * 37) % 100}%;background:{COLORS[i % 6]};animation-delay:{(i % 9) as f32 * 0.18}s",
                    }
                }
            }
            div { class: "relative flex-1 flex flex-col items-center justify-center gap-3 px-8 text-center",
                div { class: "size-56 motion-safe:animate-hop", dangerous_inner_html: "{svg}" }
                h1 { class: "text-3xl font-semibold tracking-tight", "All {planned} done!" }
                p { class: "text-lg text-stone-700", "{name} is very proud of you." }
            }
            // Stacked: the main action at the bottom, under the thumb.
            div { class: "relative p-4 flex flex-col gap-2",
                button { class: "{SECONDARY} w-full !h-12", onclick: move |_| celebrate.set(None), "Enjoy the rest of the day" }
                button {
                    class: "{PRIMARY} w-full !h-12",
                    onclick: move |_| {
                        celebrate.set(None);
                        nav.set(Nav { tab: Tab::Today, pick_more: true, ..Nav::default() });
                    },
                    Icon { name: "add_task", class: "!text-[20px]" }
                    "Pick more"
                }
            }
        }
    }
}

/// One of four drawn kittens (bundled, offline), and its name.
fn kitten_svg(which: usize) -> (String, &'static str) {
    let (fur, dark, inner, name, light) = [
        ("#fdba74", "#f97316", "#fecdd3", "Pumpkin", false),
        ("#d6d3d1", "#a8a29e", "#fbcfe8", "Pebble", false),
        ("#44403c", "#292524", "#f9a8d4", "Shadow", true),
        ("#fef3c7", "#fbbf24", "#fecdd3", "Honey", false),
    ][which % 4];
    let eye = if light { "#fef08a" } else { "#292524" };
    let line = if light { "#e7e5e4" } else { "#57534e" };
    let whisker = if light { "#e7e5e4" } else { "#78716c" };
    let stripes = if matches!(which % 4, 0 | 3) {
        format!(
            r#"<path d="M100 52 v14 M86 56 l3 11 M114 56 l-3 11" stroke="{dark}" stroke-width="4" stroke-linecap="round"/>"#
        )
    } else {
        String::new()
    };
    let svg = format!(
        r##"<svg viewBox="0 0 200 200" width="100%" height="100%" role="img" aria-label="{name}, a happy kitten">
<path d="M150 160 C185 150 190 110 168 100" fill="none" stroke="{fur}" stroke-width="14" stroke-linecap="round"/>
<ellipse cx="100" cy="150" rx="52" ry="38" fill="{fur}"/>
<ellipse cx="100" cy="160" rx="28" ry="20" fill="#ffffff" opacity=".55"/>
<path d="M50 70 L58 18 L88 48 Z" fill="{fur}"/><path d="M150 70 L142 18 L112 48 Z" fill="{fur}"/>
<path d="M58 60 L62 32 L80 50 Z" fill="{inner}"/><path d="M142 60 L138 32 L120 50 Z" fill="{inner}"/>
<circle cx="100" cy="88" r="52" fill="{fur}"/>{stripes}
<path d="M70 86 q10 -12 20 0" fill="none" stroke="{eye}" stroke-width="5" stroke-linecap="round"/>
<path d="M110 86 q10 -12 20 0" fill="none" stroke="{eye}" stroke-width="5" stroke-linecap="round"/>
<circle cx="68" cy="104" r="9" fill="#fb7185" opacity=".45"/><circle cx="132" cy="104" r="9" fill="#fb7185" opacity=".45"/>
<path d="M95 98 h10 l-5 6 Z" fill="#f472b6"/>
<path d="M100 104 q-6 8 -12 3 M100 104 q6 8 12 3" fill="none" stroke="{line}" stroke-width="2.5" stroke-linecap="round"/>
<g stroke="{whisker}" stroke-width="2" stroke-linecap="round" opacity=".8"><path d="M60 100 h-24 M62 108 l-22 6 M140 100 h24 M138 108 l22 6"/></g>
<ellipse cx="78" cy="178" rx="14" ry="9" fill="{fur}"/><ellipse cx="122" cy="178" rx="14" ry="9" fill="{fur}"/>
<path d="M150 34 l4 -9 l4 9 l9 4 l-9 4 l-4 9 l-4 -9 l-9 -4 Z" fill="#facc15"/>
<path d="M36 128 c-6 -8 4 -14 8 -6 c4 -8 14 -2 8 6 l-8 8 Z" fill="#f472b6"/>
</svg>"##
    );
    (svg, name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moving_a_task_puts_it_where_it_was_dropped() {
        let (a, b, c) = (Uuid::from_u128(1), Uuid::from_u128(2), Uuid::from_u128(3));
        let mut ids = vec![a, b, c];
        place_at(&mut ids, c, 0);
        assert_eq!(ids, vec![c, a, b]);
        place_at(&mut ids, c, 9);
        assert_eq!(ids, vec![a, b, c]);
    }

    #[test]
    fn a_row_reference_names_its_kind_and_id() {
        let id = Uuid::from_u128(7);
        assert_eq!(reference(&format!("p:{id}")), Some(('p', id)));
        assert_eq!(reference("nonsense"), None);
    }
}
