//! The app's screens.
//!
//! - [`onboarding`]: creating a family or joining one
//! - [`home`]: the frame around a family: color scheme, top bar, bottom bar,
//!   navigation, toasts and sheets, and where you are ([`home::Nav`])
//! - [`tasks`], [`places`], [`family`]: the three tabs
//! - [`detail`]: one task in focus, full screen
//! - [`addbar`]: the bar at the bottom that adds a task, group, place or location
//! - [`sheets`]: small dialogs (a name, who does a task, the invitation)

mod addbar;
mod detail;
mod family;
mod home;
mod onboarding;
mod places;
mod sheets;
mod tasks;

use dioxus::prelude::*;

pub use home::Home;
pub use onboarding::Onboarding;

/// Shown instead of the app when its data cannot be opened.
#[component]
pub fn StartupFailed(message: String) -> Element {
    rsx! {
        div { class: "h-full flex flex-col items-center justify-center gap-3 p-8 text-center bg-stone-100",
            h1 { class: "text-2xl font-semibold", "Tackly can’t start" }
            p { class: "text-stone-600", "Its data could not be opened:" }
            p { class: "text-sm text-stone-600", "{message}" }
        }
    }
}
