//! The first screen: create a family, or join one with an invitation.

use dioxus::prelude::*;
use tackly_client::JoinRequest;

use crate::{platform, state::AppState};

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
    // `tackly-app tackly://join?c=...` opens straight on the join form.
    let launch_link = platform::launch_link();
    let mut mode = use_signal(|| match launch_link {
        Some(_) => Mode::Join,
        None => Mode::Choose,
    });
    let server = use_signal(platform::default_server);
    let family_name = use_signal(String::new);
    let my_name = use_signal(String::new);
    let mut invitation = use_signal(|| launch_link.unwrap_or_default());
    // Set while this phone waits for the head of the family to let it in.
    let mut join_request = use_signal(|| None::<JoinRequest>);
    let busy = use_signal(|| false);

    rsx! {
        div { class: "welcome",
            div { class: "logo", "🌱" }
            h1 { "Tackly" }
            match mode() {
                Mode::Choose => rsx! {
                    p { "Small tasks for the whole family. Works offline, stays in sync." }
                    button { class: "btn", onclick: move |_| mode.set(Mode::Create), "Create a family" }
                    button { class: "btn tonal", onclick: move |_| mode.set(Mode::Join), "Join with an invitation" }
                },
                Mode::Create => rsx! {
                    TextField { id: "create-name", label: "Your name", value: my_name }
                    TextField { id: "create-family", label: "Family name", value: family_name }
                    TextField { id: "create-server", label: "Server", value: server }
                    p { class: "meta", "No server right now? Tasks work anyway and sync when it is reachable." }
                    div { class: "row",
                        button { class: "btn text", onclick: move |_| mode.set(Mode::Choose), "Back" }
                        button {
                            class: "btn",
                            disabled: busy(),
                            onclick: move |_| create_family(state, server, family_name, my_name, busy),
                            "Create"
                        }
                    }
                },
                Mode::Join => rsx! {
                    if let Some(request) = join_request() {
                        p { "Compare this code with the head of the family's phone, then wait for approval." }
                        div { class: "code", "{request.confirmation}" }
                        div { class: "row",
                            button { class: "btn text", onclick: move |_| join_request.set(None), "Cancel" }
                        }
                    } else {
                        TextField { id: "join-name", label: "Your name", value: my_name }
                        div { class: "field",
                            label { r#for: "join-link", "Invitation link" }
                            textarea {
                                id: "join-link",
                                value: "{invitation}",
                                oninput: move |event| invitation.set(event.value()),
                            }
                        }
                        p { class: "meta",
                            "Paste the link you were sent. Scanning the QR code with the camera comes with the Android build."
                        }
                        div { class: "row",
                            button { class: "btn text", onclick: move |_| mode.set(Mode::Choose), "Back" }
                            button {
                                class: "btn",
                                disabled: busy(),
                                onclick: move |_| join_family(state, invitation, my_name, join_request, busy),
                                "Ask to join"
                            }
                        }
                    }
                },
            }
        }
    }
}

/// A labelled one-line text input bound to a signal.
#[component]
fn TextField(id: String, label: String, mut value: Signal<String>) -> Element {
    rsx! {
        div { class: "field",
            label { r#for: "{id}", "{label}" }
            input {
                id: "{id}",
                value: "{value}",
                oninput: move |event| value.set(event.value()),
            }
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
