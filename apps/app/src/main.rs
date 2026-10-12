//! Tackly: shared family tasks that work offline and sync live.

mod platform;
mod screens;
mod settings;
mod state;
mod theme;
mod ui;
#[cfg(feature = "ui-test")]
mod uitest;

use dioxus::prelude::*;
use state::AppState;

/// The window the desktop build opens: phone-shaped, since the app is made
/// for phones.
#[cfg(feature = "desktop")]
fn launch() {
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
    // The acceptance tests, run on someone's computer (`TACKLY_QUIET`), open
    // several windows that are never shown, never take the keyboard focus and
    // (on macOS) do not even activate the app, so the person at the computer
    // can go on working. On the CI runner nobody works, and windows that are
    // never shown do not load there, so they are shown.
    #[cfg(feature = "ui-test")]
    let quiet = std::env::var_os("TACKLY_QUIET").is_some();
    #[cfg(feature = "ui-test")]
    let window = if quiet {
        window
            .with_focused(false)
            .with_always_on_bottom(true)
            .with_visible(false)
    } else {
        window
    };
    let config = Config::new().with_window(window).with_menu(None);
    #[cfg(all(feature = "ui-test", target_os = "macos"))]
    let config = if quiet {
        use dioxus::desktop::tao::{
            event_loop::EventLoopBuilder,
            platform::macos::{ActivationPolicy, EventLoopExtMacOS},
        };

        let mut events = EventLoopBuilder::with_user_event().build();
        events.set_activation_policy(ActivationPolicy::Accessory);
        events.set_activate_ignoring_other_apps(false);
        config.with_event_loop(events)
    } else {
        config
    };
    dioxus::LaunchBuilder::desktop()
        .with_cfg(config)
        .launch(App);
}

#[cfg(not(feature = "desktop"))]
fn launch() {
    dioxus::launch(App);
}

fn main() {
    launch();
}

#[component]
fn App() -> Element {
    #[cfg(feature = "ui-test")]
    uitest::install();
    let opened = use_hook(|| AppState::open().map_err(|error| format!("{error:#}")));
    let fonts = use_hook(ui::font_faces);
    rsx! {
        style { {fonts} }
        style { {include_str!("style.css")} }
        div { class: "h-dvh max-w-[430px] mx-auto bg-stone-300 font-sans antialiased",
            match opened {
                Ok(state) => rsx! { Ready { state } },
                Err(message) => rsx! { screens::StartupFailed { message } },
            }
        }
    }
}

/// The app once its data is open: the onboarding before a family exists, the
/// family's screens after.
#[component]
fn Ready(state: AppState) -> Element {
    use_context_provider(|| state);
    state.start_sync();
    let in_a_family = (state.membership)().is_some();
    rsx! {
        if in_a_family { screens::Home {} } else { screens::Onboarding {} }
    }
}
