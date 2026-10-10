//! The Activity tab: what happened in the family, newest first.

use dioxus::prelude::*;
use tackly_protocol::{Activity, ActivityKind};

use super::format::ago;
use crate::state::AppState;

#[component]
pub fn ActivityTab() -> Element {
    let state = use_context::<AppState>();
    let (family, now) = ((state.family)(), (state.now)());
    rsx! {
        for entry in family.activity.iter().rev() {
            {
                let (icon, text) = describe(entry, family.member_name(entry.by));
                rsx! {
                    ActivityCard {
                        key: "{entry.event_id}",
                        icon: icon.to_owned(),
                        text,
                        when: ago(now, entry.at),
                    }
                }
            }
        }
    }
}

#[component]
fn ActivityCard(icon: String, text: String, when: String) -> Element {
    rsx! {
        div { class: "card",
            div { class: "emoji", aria_hidden: "true", "{icon}" }
            div { class: "body",
                div { class: "title", style: "font-size:15px", "{text}" }
                div { class: "meta", "{when}" }
            }
        }
    }
}

/// An icon and a sentence for one thing that happened.
fn describe(entry: &Activity, who: &str) -> (&'static str, String) {
    let subject = &entry.subject;
    match entry.kind {
        ActivityKind::FamilyCreated => ("🌱", format!("{who} created {subject}")),
        ActivityKind::Joined => ("👋", format!("{subject} joined the family")),
        ActivityKind::Created => ("➕", format!("{who} added “{subject}”")),
        ActivityKind::Started => ("▶️", format!("{who} started “{subject}”")),
        ActivityKind::Finished => ("✅", format!("{who} finished “{subject}”")),
        ActivityKind::Reopened => ("↩️", format!("{who} reopened “{subject}”")),
        ActivityKind::Conflicted => ("⚠️", format!("{who} also finished “{subject}”")),
        ActivityKind::AutoSettled => (
            "🤝",
            format!("{who} also finished “{subject}”, same result"),
        ),
        ActivityKind::Resolved => ("🤝", format!("{who} settled “{subject}”")),
    }
}
