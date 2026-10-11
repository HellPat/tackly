//! The Family tab: everyone in the family. Opening someone shows the tasks they
//! are on; opening yourself shows your settings (name, picture, color scheme).

use dioxus::prelude::*;
use tackly_protocol::{Picture, Task};

use super::{home::Nav, tasks::TaskCard};
use crate::{
    state::AppState,
    theme::SCHEMES,
    ui::{Avatar, CARD, FIELD, Heading, Icon, PICTURE_ICONS, Size, TINTS},
};

#[component]
pub fn FamilyTab() -> Element {
    let state = use_context::<AppState>();
    let mut nav = use_context::<Signal<Nav>>();
    let (family, me) = ((state.family)(), (state.my_id)());
    let here = nav();

    if here.settings {
        return rsx! { Settings {} };
    }
    if let Some(person) = here.person {
        let tasks: Vec<Task> = family
            .open_tasks()
            .filter(|task| task.assignee == Some(person))
            .cloned()
            .collect();
        let name = family.member_name(person).to_owned();
        return rsx! {
            if tasks.is_empty() {
                p { class: "py-16 text-center text-stone-700", "{name} is not on any task." }
            } else {
                ul { class: "pt-1",
                    for task in tasks {
                        TaskCard { key: "{task.id}", task }
                    }
                }
            }
        };
    }
    rsx! {
        ul { class: "{CARD} overflow-hidden mt-1",
            for member in family.members.values() {
                li { key: "{member.device_id}",
                    button {
                        class: "w-full flex items-center gap-4 px-4 min-h-16 text-left hover:bg-stone-50 active:bg-stone-100",
                        onclick: {
                            let who = member.device_id;
                            move |_| {
                                let mut nav = nav.write();
                                nav.person = Some(who);
                                nav.settings = who == me;
                            }
                        },
                        Avatar { name: member.name.clone(), picture: member.picture.clone(), size: Size::Medium }
                        span { class: "flex-1 text-base", "{member.name}" }
                        if member.device_id == me {
                            span { class: "text-sm text-stone-600", "Settings" }
                        } else {
                            match family.open_tasks().filter(|task| task.assignee == Some(member.device_id)).count() {
                                0 => rsx! {},
                                1 => rsx! { span { class: "text-sm text-stone-600", "1 task" } },
                                n => rsx! { span { class: "text-sm text-stone-600", "{n} tasks" } },
                            }
                        }
                        Icon { name: "chevron_right", class: "text-stone-500" }
                    }
                }
            }
        }
    }
}

/// Your settings: name, picture, color scheme. Name and picture are seen by
/// the family; the color scheme stays on this phone.
#[component]
fn Settings() -> Element {
    let state = use_context::<AppState>();
    let (family, me) = ((state.family)(), (state.my_id)());
    let Some(myself) = family.members.get(&me).cloned() else {
        return rsx! {};
    };
    let current = (state.settings)().scheme;
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
            div { class: "grid grid-cols-4 gap-3", role: "radiogroup", aria_label: "Picture",
                for (i, icon) in PICTURE_ICONS.iter().enumerate() {
                    // With a photo, no icon is the picture.
                    button {
                        key: "{icon}",
                        role: "radio",
                        aria_checked: "{myself.picture.as_ref().is_some_and(|picture| picture.photo.is_none() && picture.icon == *icon)}",
                        aria_label: "{icon.replace('_', \" \")}",
                        class: if myself.picture.as_ref().is_some_and(|picture| picture.photo.is_none() && picture.icon == *icon) { "size-14 rounded-full grid place-items-center ring-2 ring-accent-700 ring-offset-2 ring-offset-[#fffdfb] {TINTS[i % TINTS.len()]}" } else { "size-14 rounded-full grid place-items-center {TINTS[i % TINTS.len()]}" },
                        onclick: move |_| state.set_picture(Picture { icon: (*icon).to_owned(), tint: (i % TINTS.len()) as u8, photo: None }),
                        Icon { name: *icon }
                    }
                }
            }
            div { class: "flex flex-wrap justify-center gap-2",
                PhotoButton { id: "photo-camera", label: "Take a photo", icon: "photo_camera", camera: true }
                PhotoButton { id: "photo-gallery", label: "Choose a photo", icon: "image", camera: false }
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
    if !name.trim().is_empty() && name.trim() != current {
        state.rename_me(name);
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
                        let me = *state.my_id.peek();
                        let current = state.family.peek().members.get(&me).and_then(|member| member.picture.clone());
                        let (icon, tint) = current.map_or(("sailing".to_owned(), 0), |picture| (picture.icon, picture.tint));
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
