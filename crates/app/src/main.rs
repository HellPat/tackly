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
    dioxus::LaunchBuilder::desktop()
        .with_cfg(Config::new().with_window(window).with_menu(None))
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
