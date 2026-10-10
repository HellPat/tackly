//! The app's screens, one file each.
//!
//! - [`onboarding`]: creating a family or joining one
//! - [`home`]: the frame around a family: app bar, tabs, add-a-task bar
//! - [`tasks`], [`places`], [`activity`], [`family`]: the four tabs
//! - [`place_sheets`]: forms for places and their locations
//! - [`composer`]: typing a new task, with one-tap suggestions
//! - [`sheets`]: the form for finishing a task

mod activity;
mod composer;
mod family;
mod format;
mod home;
mod onboarding;
mod place_sheets;
mod places;
mod sheets;
mod tasks;

use dioxus::prelude::*;

pub use home::Home;
pub use onboarding::Onboarding;

use crate::state::AppState;

/// The short message at the bottom, e.g. after copying the link or a failure.
#[component]
pub fn Snackbar() -> Element {
    let state = use_context::<AppState>();
    let mut snackbar = state.snackbar;
    let Some(message) = snackbar() else {
        return rsx! {};
    };
    rsx! {
        div { class: "snack",
            span { "{message}" }
            button { onclick: move |_| snackbar.set(None), "OK" }
        }
    }
}

/// Shown instead of the app when its data cannot be opened.
#[component]
pub fn StartupFailed(message: String) -> Element {
    rsx! {
        div { class: "welcome",
            div { class: "logo", "🌱" }
            h1 { "Tackly cannot start" }
            p { "Its data could not be opened:" }
            p { class: "meta", "{message}" }
        }
    }
}
