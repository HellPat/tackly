//! Small dialogs that slide up from the bottom: a name (new list, rename a list
//! or group, a new group for a place), who does a task, and the invitation.

use dioxus::prelude::*;
use tackly_client::{InviteProgress, InviteTicket};
use uuid::Uuid;

use super::home::{Nav, SheetKind};
use crate::{
    state::AppState,
    ui::{Avatar, FIELD, PRIMARY, SECONDARY, Sheet, Size},
};

/// How often the head of the family's phone asks whether an invitation moved on.
const INVITE_POLL: std::time::Duration = std::time::Duration::from_secs(1);

/// One dialog for a name: create a list or group, or rename one. Delete is
/// offered only when nothing is in it.
#[component]
pub fn NameSheet(kind: SheetKind) -> Element {
    let state = use_context::<AppState>();
    let mut sheet = use_context::<Signal<Option<SheetKind>>>();
    let mut nav = use_context::<Signal<Nav>>();
    let family = (state.family)();
    let (title, current, what) = match kind {
        SheetKind::NewList => ("New list".to_owned(), String::new(), "list"),
        SheetKind::RenameList(id) => (
            "Rename list".to_owned(),
            family
                .lists
                .get(&id)
                .map(|list| list.name.clone())
                .unwrap_or_default(),
            "list",
        ),
        SheetKind::RenameGroup(id) => (
            "Rename group".to_owned(),
            family
                .place_groups
                .get(&id)
                .map(|group| group.name.clone())
                .unwrap_or_default(),
            "group",
        ),
        SheetKind::NewGroupFor(_) => ("New group".to_owned(), String::new(), "group"),
        _ => return rsx! {},
    };
    // Deleting: only for something that exists and is empty.
    let deletable = match kind {
        SheetKind::RenameList(id) => Some(family.open_tasks_in(Some(id)).next().is_none()),
        SheetKind::RenameGroup(id) => Some(family.places_in(id).next().is_none()),
        _ => None,
    };
    let mut name = use_signal(|| current.clone());
    let field_label = if what == "list" {
        "List name"
    } else {
        "Group name"
    };
    let placeholder = if what == "list" {
        "e.g. Garden"
    } else {
        "e.g. Grocery Store"
    };
    rsx! {
        Sheet { close: move |_| sheet.set(None), label: title.clone(),
            form {
                onsubmit: move |event| {
                    event.prevent_default();
                    let typed = name().trim().to_owned();
                    sheet.set(None);
                    if typed.is_empty() {
                        return;
                    }
                    match kind {
                        SheetKind::NewList => state.create_list(typed),
                        SheetKind::RenameList(id) => state.rename_list(id, typed),
                        SheetKind::RenameGroup(id) => state.rename_group(id, typed),
                        SheetKind::NewGroupFor(place) => state.create_group_for(place, typed),
                        _ => {}
                    }
                },
                h2 { class: "text-xl font-semibold", "{title}" }
                input {
                    class: "{FIELD} mt-4",
                    aria_label: "{field_label}",
                    placeholder: "{placeholder}",
                    value: "{name}",
                    autofocus: true,
                    oninput: move |event| name.set(event.value()),
                }
                div { class: "mt-5 flex items-center gap-2",
                    match deletable {
                        Some(true) => rsx! {
                            button {
                                r#type: "button",
                                class: "mr-auto text-sm text-stone-600 underline underline-offset-4 hover:text-stone-900",
                                onclick: move |_| {
                                    sheet.set(None);
                                    match kind {
                                        SheetKind::RenameList(id) => {
                                            nav.write().list = None;
                                            state.delete_list(id);
                                        }
                                        SheetKind::RenameGroup(id) => {
                                            nav.write().group = None;
                                            state.delete_group(id);
                                        }
                                        _ => {}
                                    }
                                },
                                "Delete {what}"
                            }
                        },
                        Some(false) => rsx! {
                            span { class: "mr-auto text-sm text-stone-600",
                                if what == "list" { "Lists with open tasks can’t be deleted." } else { "Groups with places can’t be deleted." }
                            }
                        },
                        None => rsx! { span { class: "mr-auto" } },
                    }
                    button { r#type: "button", class: SECONDARY, onclick: move |_| sheet.set(None), "Cancel" }
                    button { r#type: "submit", class: PRIMARY, if deletable.is_some() { "Save" } else { "Create" } }
                }
            }
        }
    }
}

/// Who does a task: everyone in the family, you first.
#[component]
pub fn AssignSheet(task: Uuid) -> Element {
    let state = use_context::<AppState>();
    let mut sheet = use_context::<Signal<Option<SheetKind>>>();
    let (family, me) = ((state.family)(), (state.my_id)());
    let title = family
        .tasks
        .get(&task)
        .map(|task| task.title.clone())
        .unwrap_or_default();
    let mut people: Vec<_> = family.members.values().cloned().collect();
    people.sort_by_key(|member| member.device_id != me);
    rsx! {
        Sheet { close: move |_| sheet.set(None), label: "Who does {title}?",
            h2 { class: "text-xl font-semibold", "Who does “{title}”?" }
            ul { class: "mt-4 -mx-2",
                for member in people {
                    li { key: "{member.device_id}",
                        button {
                            class: "w-full flex items-center gap-4 px-2 min-h-14 rounded-lg text-left hover:bg-stone-100",
                            onclick: {
                                let who = member.device_id;
                                move |_| {
                                    sheet.set(None);
                                    state.assign_task(task, who);
                                }
                            },
                            Avatar { name: member.name.clone(), picture: member.picture.clone(), size: Size::Medium }
                            span { class: "text-base", if member.device_id == me { "Me" } else { "{member.name}" } }
                        }
                    }
                }
            }
        }
    }
}

/// The invitation: a QR code and Copy link; then the six digits to compare
/// when someone asks to join.
#[component]
pub fn InviteSheet() -> Element {
    let state = use_context::<AppState>();
    let mut sheet = use_context::<Signal<Option<SheetKind>>>();
    let ticket = use_signal(|| None::<InviteTicket>);
    let progress = use_signal(|| InviteProgress::Waiting);
    use_hook(move || {
        spawn(open_invitation(state, sheet, ticket, progress));
    });
    rsx! {
        Sheet { close: move |_| sheet.set(None), label: "Invite member",
            div { class: "flex flex-col items-center gap-5 text-center",
                h2 { class: "text-xl font-semibold", "Invite member" }
                match (ticket(), progress()) {
                    (Some(shown), InviteProgress::Requested { device_id, confirmation }) => rsx! {
                        p { class: "text-stone-700", "Someone asked to join. Do both phones show this code?" }
                        div { class: "text-4xl font-semibold tracking-[0.3em] tabular-nums text-accent-800", aria_label: "Confirmation code", "{confirmation}" }
                        div { class: "flex gap-2",
                            button { class: SECONDARY, onclick: move |_| sheet.set(None), "No" }
                            button { class: PRIMARY, onclick: move |_| state.approve_join(shown.clone(), device_id), "Yes, let them in" }
                        }
                    },
                    (Some(shown), _) => rsx! {
                        div { class: "bg-white p-3 rounded-2xl [&_svg]:size-56", role: "img", aria_label: "Invitation QR code",
                            dangerous_inner_html: tackly_client::qr::qr_svg(&shown.link),
                        }
                        p { class: "text-sm text-stone-600", "Scan with the other phone, or send the link. Works once, for 5 minutes." }
                        button { class: PRIMARY, onclick: move |_| copy_link(state, &shown.link),
                            crate::ui::Icon { name: "content_copy", class: "!text-[18px]" }
                            "Copy link"
                        }
                    },
                    (None, _) => rsx! { p { class: "text-stone-600 py-10", "Preparing the invitation…" } },
                }
            }
        }
    }
}

/// Creates an invitation, shows it, and keeps checking how it is doing until
/// someone is let in, it runs out, or the sheet closes.
async fn open_invitation(
    state: AppState,
    mut sheet: Signal<Option<SheetKind>>,
    mut ticket: Signal<Option<InviteTicket>>,
    mut progress: Signal<InviteProgress>,
) {
    let created = state.device().lock().await.create_invite().await;
    let invitation = match created {
        Ok(invitation) => invitation,
        Err(error) => {
            sheet.set(None);
            return state.say(format!("Inviting needs the server: {error:#}"));
        }
    };
    ticket.set(Some(invitation.clone()));
    loop {
        tokio::time::sleep(INVITE_POLL).await;
        if *sheet.peek() != Some(SheetKind::Invite) {
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
            Ok(InviteProgress::Approved) => {
                sheet.set(None);
                state.say("They’re in");
                state.refresh().await;
                return;
            }
            Ok(InviteProgress::Gone) => {
                sheet.set(None);
                return state.say("The invitation ran out");
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
    // Android has no desktop clipboard library: the web view copies.
    #[cfg(target_os = "android")]
    spawn({
        // The link is only URL-safe characters, so it needs no escaping.
        let script = format!(
            "const text = '{link}'; \
             try {{ await navigator.clipboard.writeText(text); dioxus.send(true); }} \
             catch (_) {{ \
               const field = document.createElement('textarea'); field.value = text; \
               document.body.appendChild(field); field.select(); \
               dioxus.send(document.execCommand('copy')); field.remove(); \
             }}"
        );
        async move {
            let copied = document::eval(&script)
                .recv::<bool>()
                .await
                .unwrap_or(false);
            state.say(if copied {
                "Link copied"
            } else {
                "Could not copy the link"
            });
        }
    });
}
