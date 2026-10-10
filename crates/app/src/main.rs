//! Tackly: shared family tasks that work offline and sync live.

mod platform;
mod screens;
mod state;
#[cfg(feature = "ui-test")]
mod uitest;

use dioxus::prelude::*;
use state::AppState;

fn main() {
    #[cfg(feature = "desktop")]
    {
        use dioxus::desktop::{Config, LogicalSize, WindowBuilder};
        let profile = std::env::var("TACKLY_PROFILE").unwrap_or_default();
        let title = if profile.is_empty() {
            "Tackly".to_owned()
        } else {
            format!("Tackly · {profile}")
        };
        let window = WindowBuilder::new()
            .with_title(title)
            .with_inner_size(LogicalSize::new(412.0, 860.0))
            .with_resizable(true);
        dioxus::LaunchBuilder::desktop()
            .with_cfg(Config::new().with_window(window).with_menu(None))
            .launch(App);
    }
    #[cfg(not(feature = "desktop"))]
    dioxus::launch(App);
}

#[component]
fn App() -> Element {
    #[cfg(feature = "ui-test")]
    uitest::install();
    let state = use_hook(|| AppState::open().expect("open Tackly data"));
    use_context_provider(|| state.clone());
    state.start_sync();
    let joined = (state.membership)().is_some();
    rsx! {
        style { {include_str!("style.css")} }
        div { class: "screen",
            if joined { screens::Home {} } else { screens::Onboarding {} }
            screens::Snackbar {}
        }
    }
}
