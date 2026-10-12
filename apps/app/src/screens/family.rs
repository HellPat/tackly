//! The Family button: the places and everyone in the family, and (for the head)
//! inviting people and AI agents. Opening someone shows the tasks they are on.
//! Your own settings are behind your picture, top right.

use dioxus::prelude::*;
use tackly_domain::{Name, Photo, Picture, Task};
use tackly_phone::RecoveryCode;
use uuid::Uuid;

use super::{
    home::{Nav, SheetKind, Tab},
    sheets::copy_text,
    tasks::TaskCard,
};
use crate::{
    state::AppState,
    theme::SCHEMES,
    ui::{Avatar, CARD, FIELD, Heading, Icon, PICTURE_ICONS, Size, TINTS},
};

#[component]
pub fn FamilyTab() -> Element {
    let state = use_context::<AppState>();
    let mut nav = use_context::<Signal<Nav>>();
    let family = (state.family)();
    let here = nav();

    if let Some(person) = here.person {
        return rsx! { Person { person } };
    }
    let membership = (state.membership)();
    let is_head = membership
        .as_ref()
        .is_some_and(|membership| membership.owner);
    let now = (state.now)();
    let agents: Vec<_> = family.agents().cloned().collect();
    let asks = (state.agent_asks)();
    let mut sheet = use_context::<Signal<Option<SheetKind>>>();
    // What an AI agent needs to ask this family for access.
    let login = membership.as_ref().map(|membership| {
        format!(
            "tackly-agent login --server {} --family {}",
            membership.server_url, membership.family_id
        )
    });
    rsx! {
        if is_head && !asks.is_empty() {
            Heading { text: "Access requests" }
            ul { class: "{CARD} overflow-hidden mb-2", "data-testid": "agent-asks",
                for ask in asks {
                    li { key: "{ask.request_id}", class: "flex items-center gap-4 px-4 min-h-16",
                        span { class: "size-10 rounded-full grid place-items-center bg-accent-100 text-accent-900", Icon { name: "smart_toy" } }
                        span { class: "flex-1 text-base", "An AI agent asks for access" }
                        button { class: crate::ui::SECONDARY, onclick: {
                            let request = ask.request_id;
                            move |_| sheet.set(Some(SheetKind::AgentAsk(request)))
                        }, "Review" }
                    }
                }
            }
        }
        ul { class: "{CARD} overflow-hidden mt-1",
            li {
                button {
                    class: "w-full flex items-center gap-4 px-4 min-h-16 text-left hover:bg-stone-50 active:bg-stone-100",
                    onclick: move |_| nav.set(Nav { tab: Tab::Places, came_from: Tab::Family, ..Nav::default() }),
                    span { class: "size-10 rounded-full grid place-items-center bg-accent-100 text-accent-900", Icon { name: "storefront" } }
                    span { class: "flex-1 text-base", "Places" }
                    Icon { name: "chevron_right", class: "text-stone-500" }
                }
            }
        }
        Heading { text: "Members" }
        ul { class: "{CARD} overflow-hidden",
            for member in family.people() {
                li { key: "{member.device_id}",
                    button {
                        class: "w-full flex items-center gap-4 px-4 min-h-16 text-left hover:bg-stone-50 active:bg-stone-100",
                        onclick: {
                            let who = member.device_id;
                            move |_| nav.write().person = Some(who)
                        },
                        Avatar { name: member.name.clone(), picture: member.picture.clone(), size: Size::Medium }
                        span { class: "flex-1 text-base", "{member.name}" }
                        match family.open_tasks().filter(|task| task.assignee == Some(member.device_id)).count() {
                            0 => rsx! {},
                            1 => rsx! { span { class: "text-sm text-stone-600", "1 task" } },
                            n => rsx! { span { class: "text-sm text-stone-600", "{n} tasks" } },
                        }
                        Icon { name: "chevron_right", class: "text-stone-500" }
                    }
                }
            }
        }
        if is_head {
            Heading { text: "AI agents" }
            div { class: "{CARD} px-4 py-4 flex flex-col gap-3",
                p { class: "text-sm text-stone-700", "An AI agent asks for access with this command. You see its request here and allow it for a time." }
                if let Some(membership) = membership.as_ref() {
                    dl { class: "text-sm grid grid-cols-[auto_1fr] gap-x-3 gap-y-1",
                        dt { class: "text-stone-600", "Server" }
                        dd { class: "font-mono break-all", aria_label: "Server", "{membership.server_url}" }
                        dt { class: "text-stone-600", "Family ID" }
                        dd { class: "font-mono break-all", aria_label: "Family ID", "{membership.family_id}" }
                    }
                }
                if let Some(command) = login {
                    button { class: "{crate::ui::SECONDARY} self-start", onclick: move |_| copy_text(state, &command, "Command copied"),
                        Icon { name: "content_copy", class: "!text-[18px]" }
                        "Copy login command"
                    }
                }
            }
        }
        if !agents.is_empty() {
            if !is_head {
                Heading { text: "AI agents" }
            }
            ul { class: "{CARD} overflow-hidden mt-2", "data-testid": "agents",
                for agent in agents {
                    li { key: "{agent.device_id}", class: "flex items-center gap-4 px-4 min-h-16 text-stone-600",
                        span { class: "size-10 rounded-full grid place-items-center bg-stone-200", Icon { name: "smart_toy" } }
                        span { class: "flex-1",
                            span { class: "block text-base", "{agent.name}" }
                            span { class: "block text-sm", "{time_left(agent.agent_until, now)}" }
                        }
                        if is_head {
                            button { class: crate::ui::SECONDARY, onclick: {
                                let who = agent.device_id;
                                move |_| state.remove_member(who)
                            }, "Revoke" }
                        }
                    }
                }
            }
        }
    }
}

/// How much longer an agent may read.
fn time_left(
    until: Option<chrono::DateTime<chrono::Utc>>,
    now: chrono::DateTime<chrono::Utc>,
) -> String {
    let Some(until) = until else {
        return String::new();
    };
    let left = until - now;
    if left <= chrono::Duration::zero() {
        "time is over".into()
    } else if left < chrono::Duration::minutes(1) {
        "less than a minute left".into()
    } else if left < chrono::Duration::hours(2) {
        format!("{} min left", (left.num_seconds() + 59) / 60)
    } else if left < chrono::Duration::days(2) {
        format!("{} h left", left.num_hours())
    } else {
        format!("{} days left", left.num_days())
    }
}

/// The recovery words, over everything: the family opens only once the head
/// says it saved them. (Right after creating the family, and after a new code.)
#[component]
pub fn RecoveryGate(code: RecoveryCode) -> Element {
    let state = use_context::<AppState>();
    let mut saved = use_signal(|| false);
    rsx! {
        div { class: "absolute inset-0 z-[60] overflow-y-auto bg-[#fffdfb]", role: "dialog", aria_modal: "true", aria_label: "Save your recovery code", "data-testid": "recovery-gate",
            div { class: "min-h-full flex flex-col justify-center gap-5 p-6 max-w-[430px] mx-auto",
                h1 { class: "text-2xl font-semibold text-center", "Save your recovery code" }
                p { class: "text-stone-700 text-center", "If this phone is lost, these 24 words bring the family back on a new phone. Without them, the family is gone with it." }
                p { class: "{CARD} px-4 py-3 font-mono text-base leading-8 break-words select-all", aria_label: "Recovery words", "{code}" }
                label { class: "flex items-center gap-3 min-h-12",
                    input {
                        r#type: "checkbox",
                        class: "size-6 accent-accent-700",
                        checked: "{saved()}",
                        onchange: move |event| saved.set(event.checked()),
                    }
                    span { class: "text-base", "I saved it" }
                }
                button { class: "{crate::ui::PRIMARY} w-full !h-12 text-base", disabled: !saved(), onclick: move |_| state.confirm_recovery_code(), "Continue" }
            }
        }
    }
}

/// Someone else: the tasks they are on. The head can take them out of the family.
#[component]
fn Person(person: Uuid) -> Element {
    let state = use_context::<AppState>();
    let mut nav = use_context::<Signal<Nav>>();
    let family = (state.family)();
    let me = (state.my_id)();
    let is_head = (state.membership)().is_some_and(|membership| membership.owner) && person != me;
    let mut removing = use_signal(|| false);
    let tasks: Vec<Task> = family
        .open_tasks()
        .filter(|task| task.assignee == Some(person))
        .cloned()
        .collect();
    let name = family.member_name(person).to_owned();
    rsx! {
        if tasks.is_empty() {
            p { class: "py-16 text-center text-stone-700", "{name} is not on any task." }
        } else {
            ul { class: "pt-1",
                for task in tasks {
                    TaskCard { key: "{task.id}", task }
                }
            }
        }
        if is_head {
            div { class: "pt-6 pb-6 text-center",
                if removing() {
                    p { class: "text-sm text-stone-700", "{name}’s phone keeps what it has and sees nothing new." }
                    div { class: "mt-3 flex justify-center gap-2",
                        button { class: crate::ui::SECONDARY, onclick: move |_| removing.set(false), "Cancel" }
                        button { class: crate::ui::SECONDARY, onclick: move |_| {
                            state.remove_member(person);
                            nav.write().person = None;
                        }, "Remove {name}" }
                    }
                } else {
                    button { class: "text-sm text-stone-700 underline underline-offset-4", onclick: move |_| removing.set(true), "Remove from family" }
                }
            }
        }
    }
}

/// Your settings: name, picture, color scheme. Name and picture are seen by
/// the family; the color scheme stays on this phone.
#[component]
pub fn SettingsTab() -> Element {
    let state = use_context::<AppState>();
    let (family, me) = ((state.family)(), (state.my_id)());
    let Some(myself) = family.members.get(&me).cloned() else {
        return rsx! {};
    };
    let current = (state.settings)().scheme;
    let plan_at = (state.settings)()
        .plan_at
        .map(|time| time.to_string())
        .unwrap_or_default();
    let mut leaving = use_signal(|| false);
    let mut typed = use_signal(|| myself.name.clone());
    rsx! {
        Heading { text: "Name" }
        input {
            class: FIELD,
            aria_label: "Your name",
            value: "{myself.name}",
            // Saved when the field is left, or on Enter.
            onchange: move |event| save_name(state, event.value()),
            onkeydown: move |event| {
                if event.key() == Key::Enter {
                    save_name(state, typed());
                }
            },
            oninput: move |event| typed.set(event.value()),
        }
        Heading { text: "Picture" }
        div { class: "{CARD} p-4 flex flex-col items-center gap-4",
            Avatar { name: myself.name.clone(), picture: myself.picture.clone(), size: Size::Huge }
            div { class: "grid grid-cols-4 gap-3", role: "radiogroup", aria_label: "Picture", "data-testid": "picture",
                for (i, icon) in PICTURE_ICONS.iter().enumerate() {
                    // With a photo, no icon is the picture.
                    button {
                        key: "{icon}",
                        role: "radio",
                        aria_checked: "{myself.picture.as_ref().is_some_and(|picture| picture.photo.is_none() && picture.icon.as_str() == *icon)}",
                        aria_label: "{icon.replace('_', \" \")}",
                        class: if myself.picture.as_ref().is_some_and(|picture| picture.photo.is_none() && picture.icon.as_str() == *icon) { "size-14 rounded-full grid place-items-center ring-2 ring-accent-700 ring-offset-2 ring-offset-[#fffdfb] {TINTS[i % TINTS.len()]}" } else { "size-14 rounded-full grid place-items-center {TINTS[i % TINTS.len()]}" },
                        onclick: move |_| {
                            // The icons here are all names of icons.
                            if let Ok(icon) = icon.parse() {
                                state.set_picture(Picture { icon, tint: (i % TINTS.len()) as u8, photo: None });
                            }
                        },
                        Icon { name: *icon }
                    }
                }
            }
            div { class: "flex flex-wrap justify-center gap-2",
                PhotoButton { id: "photo-camera", label: "Take a photo", icon: "photo_camera", camera: true }
                PhotoButton { id: "photo-gallery", label: "Choose a photo", icon: "image", camera: false }
            }
        }
        Heading { text: "Planning" }
        label { class: "{CARD} flex items-center gap-3 px-4 min-h-16",
            span { class: "flex-1",
                span { class: "block", "Plan the next day at" }
                span { class: "block text-sm text-stone-600", "Upcoming opens then. Empty: never." }
            }
            input {
                r#type: "time",
                class: "h-11 px-3 rounded-md border border-stone-300 bg-[#fffdfb] text-base",
                aria_label: "Planning time",
                value: "{plan_at}",
                // Empty (or not a time): never.
                onchange: move |event| state.set_plan_at(event.value().parse().ok()),
            }
        }
        if (state.membership)().is_some_and(|membership| membership.owner) {
            Heading { text: "Recovery code" }
            div { class: "{CARD} px-4 py-4 flex flex-col gap-3",
                p { class: "text-sm text-stone-700", "A new code replaces the one you have. The old words stop working." }
                button { class: "{crate::ui::SECONDARY} self-start", onclick: move |_| state.new_recovery_code(),
                    Icon { name: "key", class: "!text-[18px]" }
                    "New recovery code"
                }
            }
        }
        Heading { text: "Color scheme" }
        div { class: "grid grid-cols-3 gap-3 pb-4", role: "radiogroup", aria_label: "Color scheme",
            for scheme in SCHEMES {
                button {
                    key: "{scheme.id}",
                    role: "radio",
                    aria_checked: "{scheme.id == current}",
                    aria_label: "{scheme.name}",
                    class: "flex flex-col gap-1.5 text-left",
                    onclick: move |_| state.set_scheme(scheme.id),
                    span {
                        class: if scheme.id == current { "relative block h-28 rounded-xl overflow-hidden shadow-sm ring-2 ring-stone-600 ring-offset-2 ring-offset-stone-100 {scheme.sky}" } else { "relative block h-28 rounded-xl overflow-hidden shadow-sm {scheme.sky}" },
                        style: "{scheme.accent_vars()}",
                        span { class: "absolute inset-x-0 bottom-0 h-3/5", dangerous_inner_html: "{scheme.scene_svg()}" }
                        span { class: "absolute left-2 top-2 h-5 w-10 rounded-full bg-accent-700" }
                    }
                    span { class: if scheme.id == current { "text-sm font-semibold" } else { "text-sm text-stone-700" }, "{scheme.name}" }
                }
            }
        }
        div { class: "pt-2 pb-6 text-center",
            if leaving() {
                p { class: "text-sm text-stone-700", "This erases the family’s tasks from this phone. The others keep theirs." }
                div { class: "mt-3 flex justify-center gap-2",
                    button { class: crate::ui::SECONDARY, onclick: move |_| leaving.set(false), "Cancel" }
                    button { class: crate::ui::SECONDARY, onclick: move |_| state.leave_family(), "Leave the family" }
                }
            } else {
                button { class: "text-sm text-stone-700 underline underline-offset-4", onclick: move |_| leaving.set(true), "Leave the family on this phone" }
            }
        }
    }
}

fn save_name(state: AppState, name: String) {
    let current = state
        .family
        .peek()
        .member_name(*state.my_id.peek())
        .to_owned();
    if let Ok(new) = name.parse::<Name>()
        && new.as_str() != current
    {
        state.rename_me(new.to_string());
    }
}

/// A photo as your picture, from the camera or the gallery. The web view crops
/// it square and makes a small JPEG (256 px), which syncs to the family.
#[component]
fn PhotoButton(id: String, label: String, icon: String, camera: bool) -> Element {
    let state = use_context::<AppState>();
    let input_id = id.clone();
    rsx! {
        label { class: "{crate::ui::SECONDARY} cursor-pointer",
            Icon { name: icon, class: "!text-[18px]" }
            "{label}"
            input {
                id: "{id}",
                r#type: "file",
                accept: "image/*",
                class: "sr-only",
                aria_label: "{label}",
                "capture": if camera { "user" } else { "" },
                onchange: move |_| {
                    let input_id = input_id.clone();
                    spawn(async move {
                        let photo: Option<String> = document::eval(&shrink_photo_script(&input_id)).recv().await.ok().flatten();
                        let Some(photo) = photo else { return };
                        let Ok(photo) = photo.parse::<Photo>() else {
                            return state.say("That photo is not usable");
                        };
                        let me = *state.my_id.peek();
                        let current = state.family.peek().members.get(&me).and_then(|member| member.picture.clone());
                        let Some((icon, tint)) = current.map(|picture| (picture.icon, picture.tint)).or_else(|| "sailing".parse().ok().map(|icon| (icon, 0))) else { return };
                        state.set_picture(Picture { icon, tint, photo: Some(photo) });
                    });
                },
            }
        }
    }
}

/// Reads the chosen image, crops the middle square, and sends back a 256 px JPEG data URL.
fn shrink_photo_script(input_id: &str) -> String {
    format!(
        "const input = document.getElementById('{input_id}');
         const file = input && input.files[0];
         if (!file) {{ dioxus.send(null); }} else {{
           const url = URL.createObjectURL(file);
           const img = new Image();
           img.src = url;
           await img.decode();
           const side = Math.min(img.naturalWidth, img.naturalHeight);
           const canvas = document.createElement('canvas');
           canvas.width = canvas.height = 256;
           canvas.getContext('2d').drawImage(img, (img.naturalWidth - side) / 2, (img.naturalHeight - side) / 2, side, side, 0, 0, 256, 256);
           URL.revokeObjectURL(url);
           input.value = '';
           dioxus.send(canvas.toDataURL('image/jpeg', 0.85));
         }}"
    )
}
