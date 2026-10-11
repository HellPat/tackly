//! The first screen: create a family, or join one with an invitation.

use dioxus::prelude::*;
use tackly_client::JoinRequest;

use crate::{
    platform,
    state::AppState,
    theme,
    ui::{FIELD, PRIMARY, SECONDARY, Toasts},
};

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Choose,
    Create,
    Join,
}

/// How often a phone that asked to join checks whether it was let in.
const JOIN_POLL: std::time::Duration = std::time::Duration::from_secs(1);

#[component]
pub fn Onboarding() -> Element {
    let state = use_context::<AppState>();
    // `tackly tackly://join?c=...` opens straight on the join form.
    let launch_link = platform::launch_link();
    let mut mode = use_signal(|| {
        if launch_link.is_some() {
            Mode::Join
        } else {
            Mode::Choose
        }
    });
    let server = use_signal(platform::default_server);
    let family_name = use_signal(String::new);
    let my_name = use_signal(String::new);
    let invitation = use_signal(|| launch_link.unwrap_or_default());
    // Set while this phone waits for the head of the family to let it in.
    let mut join_request = use_signal(|| None::<JoinRequest>);
    let busy = use_signal(|| false);
    let scheme = theme::scheme(&(state.settings)().scheme);

    rsx! {
        div { class: "relative h-full overflow-y-auto text-stone-800 {scheme.sky}", style: "{scheme.accent_vars()}",
            div { class: "absolute inset-x-0 bottom-0 h-[42%] pointer-events-none", aria_hidden: "true", dangerous_inner_html: "{scheme.scene_svg()}" }
            div { class: "relative z-10 min-h-full flex flex-col justify-center gap-4 p-6",
                h1 { class: "text-4xl font-semibold tracking-tight text-center", "Tackly" }
                div { class: "bg-[#fffdfb] rounded-2xl shadow-sm p-5 flex flex-col gap-4",
                    match mode() {
                        Mode::Choose => rsx! {
                            p { class: "text-center text-stone-700", "ADHD optimized task management. Shared with your family, works offline." }
                            button { class: "{PRIMARY} !h-12", onclick: move |_| mode.set(Mode::Create), "Create a family" }
                            button { class: "{SECONDARY} !h-12", onclick: move |_| mode.set(Mode::Join), "Join with an invitation" }
                        },
                        Mode::Create => rsx! {
                            TextField { label: "Your name", value: my_name }
                            TextField { label: "Family name", value: family_name }
                            TextField { label: "Server", value: server }
                            p { class: "text-sm text-stone-700", "No server right now? Tasks work anyway and sync when it is reachable." }
                            div { class: "flex justify-end gap-2",
                                button { class: SECONDARY, onclick: move |_| mode.set(Mode::Choose), "Back" }
                                button { class: PRIMARY, disabled: busy(), onclick: move |_| create_family(state, server, family_name, my_name, busy), "Create" }
                            }
                        },
                        Mode::Join => rsx! {
                            if let Some(request) = join_request() {
                                p { class: "text-stone-700", "Compare this code with the phone that invited you, then wait to be let in." }
                                div { class: "text-4xl font-semibold tracking-[0.3em] tabular-nums text-center text-accent-800", aria_label: "Confirmation code", "{request.confirmation}" }
                                div { class: "flex justify-end",
                                    button { class: SECONDARY, onclick: move |_| join_request.set(None), "Cancel" }
                                }
                            } else {
                                TextField { label: "Your name", value: my_name }
                                TextField { label: "Invitation link", value: invitation }
                                p { class: "text-sm text-stone-700", "Paste the link you were sent." }
                                div { class: "flex justify-end gap-2",
                                    button { class: SECONDARY, onclick: move |_| mode.set(Mode::Choose), "Back" }
                                    button { class: PRIMARY, disabled: busy(), onclick: move |_| join_family(state, invitation, my_name, join_request, busy), "Ask to join" }
                                }
                            }
                        },
                    }
                }
            }
            Toasts {}
        }
    }
}

/// A labelled one-line text input bound to a signal.
#[component]
fn TextField(label: String, mut value: Signal<String>) -> Element {
    rsx! {
        label { class: "block",
            span { class: "text-sm font-semibold text-stone-700", "{label}" }
            input { class: "{FIELD} mt-1.5", value: "{value}", oninput: move |event| value.set(event.value()) }
        }
    }
}

async fn create_family(
    state: AppState,
    server: Signal<String>,
    family_name: Signal<String>,
    my_name: Signal<String>,
    mut busy: Signal<bool>,
) {
    busy.set(true);
    let created = state
        .device()
        .lock()
        .await
        .create_family(&server(), &family_name(), &my_name())
        .await;
    if let Err(error) = created {
        state.say(format!("{error:#}"));
    }
    busy.set(false);
    state.refresh().await;
}

/// Asks to join, shows the confirmation digits, and keeps checking until the
/// head of the family has let this phone in (or the person cancels).
async fn join_family(
    state: AppState,
    invitation: Signal<String>,
    my_name: Signal<String>,
    mut join_request: Signal<Option<JoinRequest>>,
    mut busy: Signal<bool>,
) {
    busy.set(true);
    let asked = state
        .device()
        .lock()
        .await
        .request_join(&invitation())
        .await;
    busy.set(false);
    let request = match asked {
        Ok(request) => request,
        Err(error) => return state.say(format!("{error:#}")),
    };
    join_request.set(Some(request.clone()));
    while join_request.peek().is_some() {
        let joined = state
            .device()
            .lock()
            .await
            .complete_join(&request, &my_name())
            .await;
        match joined {
            Ok(true) => break,
            Ok(false) => tokio::time::sleep(JOIN_POLL).await,
            Err(error) => {
                join_request.set(None);
                return state.say(format!("{error:#}"));
            }
        }
    }
    join_request.set(None);
    state.refresh().await;
}
