//! The Family tab: who is in the family, inviting someone, leaving.

use dioxus::prelude::*;
use tackly_client::{InviteProgress, InviteTicket};

use super::format::initial;
use crate::state::AppState;

/// How often the head of the family's phone asks whether an invitation moved on.
const INVITE_POLL: std::time::Duration = std::time::Duration::from_secs(1);

#[component]
pub fn FamilyTab() -> Element {
    let state = use_context::<AppState>();
    let (family, me) = ((state.family)(), (state.my_id)());
    let is_head = (state.membership)().is_some_and(|membership| membership.owner);
    let mut asking_to_leave = use_signal(|| false);
    rsx! {
        div { class: "section", "Members" }
        for member in family.members.values() {
            div { key: "{member.device_id}", class: "card", style: "align-items:center",
                div { class: "avatar", aria_hidden: "true", "{initial(&member.name)}" }
                div { class: "body",
                    div { class: "title",
                        "{member.name}"
                        if member.device_id == me {
                            " (you)"
                        }
                    }
                    div { class: "meta",
                        if member.owner { "Head of the family" } else { "Member" }
                    }
                }
            }
        }
        if is_head {
            div { class: "section", "Invite" }
            InvitePanel {}
        }
        div { class: "section", "This phone" }
        button { class: "btn danger", onclick: move |_| asking_to_leave.set(true), "Leave family on this phone" }
        if asking_to_leave() {
            LeaveDialog { close: move |_| asking_to_leave.set(false) }
        }
    }
}

/// Inviting someone: a button, and once pressed the QR code, the link, and
/// the two phones' confirmation digits.
#[component]
fn InvitePanel() -> Element {
    let state = use_context::<AppState>();
    let mut ticket = use_signal(|| None::<InviteTicket>);
    let progress = use_signal(|| InviteProgress::Waiting);
    let Some(shown) = ticket() else {
        return rsx! {
            button {
                class: "btn tonal",
                onclick: move |_| {
                    spawn(open_invitation(state, ticket, progress));
                },
                "Invite someone"
            }
        };
    };
    rsx! {
        div { class: "card", style: "flex-direction:column",
            div { class: "meta",
                "Let the other phone scan this code, or copy the link and send it by message. It expires in 5 minutes."
            }
            div {
                class: "qr",
                role: "img",
                aria_label: "Invitation QR code",
                dangerous_inner_html: tackly_client::qr::qr_svg(&shown.link),
            }
            div { class: "actions",
                button {
                    class: "btn tonal",
                    onclick: {
                        let link = shown.link.clone();
                        move |_| copy_link(state, &link)
                    },
                    "Copy link"
                }
            }
            div { class: "field",
                label { r#for: "invite-link", "Invitation link" }
                textarea { id: "invite-link", readonly: true, value: "{shown.link}" }
            }
            match progress() {
                InviteProgress::Requested { device_id, confirmation } => rsx! {
                    div { class: "meta", "Someone asked to join. Do both phones show this code?" }
                    div { class: "code", "{confirmation}" }
                    div { class: "actions",
                        button {
                            class: "btn",
                            onclick: {
                                let shown = shown.clone();
                                move |_| state.approve_join(shown.clone(), device_id)
                            },
                            "Yes, let them in"
                        }
                        button { class: "btn text", onclick: move |_| ticket.set(None), "No" }
                    }
                },
                _ => rsx! {
                    div { class: "meta", "Waiting for the other phone…" }
                },
            }
            div { class: "actions",
                button { class: "btn text", onclick: move |_| ticket.set(None), "Close" }
            }
        }
    }
}

/// Creates an invitation, shows it, and keeps checking how it is doing until
/// someone is let in, it runs out, or the person closes it.
async fn open_invitation(
    state: AppState,
    mut ticket: Signal<Option<InviteTicket>>,
    mut progress: Signal<InviteProgress>,
) {
    let created = state.device().lock().await.create_invite().await;
    let invitation = match created {
        Ok(invitation) => invitation,
        Err(error) => return state.say(format!("Sharing needs the server: {error:#}")),
    };
    progress.set(InviteProgress::Waiting);
    ticket.set(Some(invitation.clone()));
    loop {
        tokio::time::sleep(INVITE_POLL).await;
        let still_shown = ticket.peek().as_ref().map(|shown| shown.invite_id);
        if still_shown != Some(invitation.invite_id) {
            return;
        }
        // Read in its own statement: a lock taken in a `match` head would be
        // held through the arms, and `refresh` locks the device again.
        let latest = state
            .device()
            .lock()
            .await
            .invite_progress(&invitation)
            .await;
        match latest {
            Ok(InviteProgress::Approved | InviteProgress::Gone) => {
                ticket.set(None);
                state.refresh().await;
                return;
            }
            Ok(next) => progress.set(next),
            // A hiccup in the connection: try again next time.
            Err(_) => {}
        }
    }
}

/// Puts the invitation link on the clipboard.
fn copy_link(state: AppState, link: &str) {
    #[cfg(not(target_os = "android"))]
    match arboard::Clipboard::new().and_then(|mut clipboard| clipboard.set_text(link.to_owned())) {
        Ok(()) => state.say("Link copied"),
        Err(error) => state.say(format!("Could not copy: {error}")),
    }
    #[cfg(target_os = "android")]
    {
        let _ = link;
        state.say("Select the link and copy it");
    }
}

#[component]
fn LeaveDialog(close: EventHandler<()>) -> Element {
    let state = use_context::<AppState>();
    rsx! {
        div { class: "scrim", onclick: move |_| close.call(()),
            div { class: "dialog", onclick: move |event| event.stop_propagation(),
                h2 { "Leave this family?" }
                p { "This erases the family's tasks from this phone. The others keep theirs." }
                div { class: "row",
                    button { class: "btn text", onclick: move |_| close.call(()), "Cancel" }
                    button {
                        class: "btn danger",
                        onclick: move |_| {
                            close.call(());
                            state.leave_family();
                        },
                        "Leave"
                    }
                }
            }
        }
    }
}
