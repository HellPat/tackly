//! The building blocks every screen uses: buttons, cards, icons, pictures,
//! sheets, headings, rows, filters, toasts. Styled with Tailwind classes only.

use base64::Engine;
use dioxus::prelude::*;
use tackly_protocol::Picture;

use crate::{settings::Filter, state::AppState};

// ---- class names ---------------------------------------------------------------------------

/// Buttons, GitHub style: small radius, 1 px border, semibold. Primary is filled
/// with the accent; secondary is neutral.
pub const PRIMARY: &str = "inline-flex items-center justify-center gap-2 h-10 px-4 rounded-md border font-semibold shadow-sm transition-colors active:shadow-none focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-accent-700 disabled:opacity-50 border-accent-800 bg-accent-700 text-white inset-shadow-[0_1px_0_rgb(255_255_255/.18)] hover:bg-accent-800";
pub const SECONDARY: &str = "inline-flex items-center justify-center gap-2 h-10 px-4 rounded-md border font-semibold shadow-sm transition-colors active:shadow-none focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-accent-700 disabled:opacity-50 border-stone-300 bg-stone-50 text-stone-800 inset-shadow-[0_1px_0_white] hover:bg-stone-100";
/// A white card on the page: lighter than the background, so it feels closer.
pub const CARD: &str = "bg-[#fffdfb] rounded-2xl shadow-sm";
/// A text field.
pub const FIELD: &str = "w-full h-12 px-4 rounded-md bg-[#fffdfb] border border-stone-300 shadow-sm text-base outline-none focus:ring-2 focus:ring-accent-700";
/// Chips (filters, groups): the active one dark grey, the others white.
pub const CHIP_ON: &str = "shrink-0 inline-flex items-center gap-1.5 h-9 px-3.5 rounded-full text-sm font-semibold transition-colors bg-stone-600 text-white";
pub const CHIP_OFF: &str = "shrink-0 inline-flex items-center gap-1.5 h-9 px-3.5 rounded-full text-sm font-semibold transition-colors bg-[#fffdfb] text-stone-800 shadow-sm";
/// Soft tints for people's pictures (written out so Tailwind finds them).
pub const TINTS: [&str; 5] = [
    "bg-sky-100 text-sky-900",
    "bg-pink-100 text-pink-900",
    "bg-teal-100 text-teal-900",
    "bg-amber-100 text-amber-900",
    "bg-violet-100 text-violet-900",
];
/// The pictures one can pick.
pub const PICTURE_ICONS: [&str; 8] = [
    "sailing",
    "local_florist",
    "pets",
    "rocket_launch",
    "sports_soccer",
    "music_note",
    "coffee",
    "bolt",
];
/// Material Symbols' filled variant (outlined otherwise).
pub const FILLED: &str = "[font-variation-settings:'FILL'_1]";

/// The two fonts, embedded so the app looks right without a network.
pub fn font_faces() -> String {
    let encode = |bytes: &[u8]| base64::engine::general_purpose::STANDARD.encode(bytes);
    let roboto = encode(include_bytes!("../fonts/roboto-latin.woff2"));
    let symbols = encode(include_bytes!("../fonts/material-symbols-rounded.woff2"));
    format!(
        "@font-face{{font-family:'Roboto';font-weight:400 600;src:url(data:font/woff2;base64,{roboto}) format('woff2')}}\
         @font-face{{font-family:'Material Symbols Rounded';font-weight:400;font-display:block;src:url(data:font/woff2;base64,{symbols}) format('woff2')}}"
    )
}

// ---- components ------------------------------------------------------------------------------

#[component]
pub fn Icon(name: String, #[props(default)] class: String) -> Element {
    rsx! { span { class: "material-symbols-rounded {class}", aria_hidden: "true", "{name}" } }
}

#[derive(Clone, Copy, PartialEq)]
pub enum Size {
    Small,
    Medium,
    Huge,
}

/// A person's picture: their icon on a tint, or the first letter of their name.
/// Round, with a white border and a small shadow, so it sits cleanly on any background.
#[component]
pub fn Avatar(name: String, picture: Option<Picture>, size: Size) -> Element {
    let (box_class, icon_class, text_class) = match size {
        Size::Small => ("size-8 border-2", "!text-[18px]", "text-sm"),
        Size::Medium => ("size-10 border-[3px]", "", "text-base"),
        Size::Huge => ("size-24 border-4", "!text-[40px]", "text-3xl"),
    };
    let tint = picture
        .as_ref()
        .map(|picture| usize::from(picture.tint))
        .unwrap_or_else(|| name.bytes().map(usize::from).sum());
    let tint = TINTS[tint % TINTS.len()];
    let initial = name
        .chars()
        .next()
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_default();
    rsx! {
        span { class: "shrink-0 rounded-full grid place-items-center overflow-hidden border-white shadow-[0_1px_3px_rgb(41_37_36/.3)] {box_class} {tint}",
            match picture {
                Some(Picture { photo: Some(photo), .. }) => rsx! { img { class: "size-full object-cover", src: "{photo}", alt: "" } },
                Some(picture) => rsx! { Icon { name: picture.icon, class: icon_class } },
                None => rsx! { span { class: "font-semibold {text_class}", aria_hidden: "true", "{initial}" } },
            }
        }
    }
}

/// A bottom sheet over a dimmed screen. Tapping outside closes it.
#[component]
pub fn Sheet(close: EventHandler<()>, label: String, children: Element) -> Element {
    rsx! {
        div { class: "fixed inset-0 z-50 flex items-end bg-stone-900/40", onclick: move |_| close.call(()),
            div {
                class: "w-full max-w-[430px] mx-auto rounded-t-[28px] bg-[#fffdfb] px-6 pt-3 pb-8 shadow-2xl",
                role: "dialog",
                aria_modal: "true",
                aria_label: "{label}",
                onclick: move |event| event.stop_propagation(),
                div { class: "mx-auto mb-5 h-1 w-8 rounded-full bg-stone-300" }
                {children}
            }
        }
    }
}

#[component]
pub fn Heading(text: String) -> Element {
    rsx! { h2 { class: "px-2 pt-6 pb-2 text-sm font-semibold text-stone-600", "{text}" } }
}

/// Nothing here: a normal-size icon in a tinted circle and a short friendly line.
#[component]
pub fn EmptyState(icon: String, title: String, line: String) -> Element {
    rsx! {
        div { class: "flex flex-col items-center text-center gap-2 py-20 px-8",
            span { class: "size-16 mb-2 rounded-full grid place-items-center bg-accent-100 text-accent-800",
                Icon { name: icon, class: "!text-[32px]" }
            }
            span { class: "text-lg font-semibold", "{title}" }
            span { class: "text-stone-600", "{line}" }
        }
    }
}

/// One row of an overview (lists, groups, places, people): icon, name, an
/// optional second line, a count, and a chevron.
#[component]
pub fn OverviewRow(
    icon: String,
    name: String,
    #[props(default)] count: String,
    #[props(default)] sub: String,
    onclick: EventHandler<()>,
) -> Element {
    rsx! {
        li {
            button {
                class: "w-full flex items-center gap-3 px-4 min-h-16 text-left hover:bg-stone-50 active:bg-stone-100",
                onclick: move |_| onclick.call(()),
                span { class: "size-9 shrink-0 rounded-full grid place-items-center bg-stone-100 text-stone-600",
                    Icon { name: icon, class: "!text-[18px]" }
                }
                span { class: "flex-1 min-w-0",
                    span { class: "block text-base truncate", "{name}" }
                    if !sub.is_empty() {
                        span { class: "block text-sm text-stone-600", "{sub}" }
                    }
                }
                if !count.is_empty() {
                    span { class: "text-sm text-stone-600", "{count}" }
                }
                Icon { name: "chevron_right", class: "text-stone-500" }
            }
        }
    }
}

/// Mine / Unassigned / All, each with its count. One setting for the whole app.
#[component]
pub fn FilterBar(counts: [usize; 3]) -> Element {
    let state = use_context::<AppState>();
    let active = (state.settings)().filter;
    let chips = [
        (Filter::Mine, "Mine"),
        (Filter::Unassigned, "Unassigned"),
        (Filter::All, "All"),
    ];
    rsx! {
        div { class: "flex gap-2 pt-1 pb-3", role: "group", aria_label: "Filter",
            for (i, (filter, label)) in chips.into_iter().enumerate() {
                button {
                    key: "{label}",
                    class: if active == filter { CHIP_ON } else { CHIP_OFF },
                    aria_pressed: "{active == filter}",
                    aria_label: "{label} ({counts[i]})",
                    onclick: move |_| state.set_filter(filter),
                    "{label}"
                    span { class: if active == filter { "font-normal text-stone-200" } else { "font-normal text-stone-500" }, "{counts[i]}" }
                }
            }
        }
    }
}

/// Toasts, stacked like Sonner: the newest in front, older ones tucked behind
/// and a little smaller; touching the stack fans it out and pauses the timers.
#[component]
pub fn Toasts() -> Element {
    let state = use_context::<AppState>();
    let toasts = (state.toasts)();
    let mut fanned = state.toasts_fanned;
    rsx! {
        section {
            class: "absolute inset-x-4 bottom-40 z-40 h-0",
            aria_label: "Notifications",
            aria_live: "polite",
            onmouseenter: move |_| fanned.set(true),
            onmouseleave: move |_| fanned.set(false),
            for (i, toast) in toasts.iter().rev().enumerate() {
                div {
                    key: "{toast.id}",
                    role: "status",
                    class: "absolute inset-x-0 bottom-0 mx-auto max-w-[360px] flex items-center gap-3 h-16 pl-4 pr-3 rounded-xl bg-white border border-stone-200 shadow-lg text-sm text-stone-800 origin-bottom transition-all duration-300 ease-out",
                    style: "{toast_position(i, fanned())}",
                    span { class: "flex-1 min-w-0 truncate", "{toast.text}" }
                    if let Some(undo) = toast.undo.clone() {
                        button {
                            class: "{SECONDARY} !h-10 !px-3",
                            onclick: {
                                let id = toast.id;
                                move |_| state.undo(id, undo.clone())
                            },
                            Icon { name: "undo", class: "!text-[20px]" }
                            "Undo"
                        }
                    }
                }
            }
        }
    }
}

/// Where the toast `i` places from the front sits (inline, since it is computed).
fn toast_position(i: usize, fanned: bool) -> String {
    const GAP: usize = 72;
    let z = 30_usize.saturating_sub(i);
    match (i, fanned) {
        (3.., _) => format!(
            "z-index:{z};opacity:0;transform:translateY(-20px) scale(.85);pointer-events:none"
        ),
        (_, true) => format!("z-index:{z};transform:translateY(-{}px)", i * GAP),
        (0, false) => format!("z-index:{z}"),
        (1, false) => format!("z-index:{z};transform:translateY(-10px) scale(.95)"),
        (_, false) => format!("z-index:{z};transform:translateY(-20px) scale(.9)"),
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeSet, path::Path};

    /// Every icon the app shows. The embedded icon font holds exactly these;
    /// see `fonts/README.md` when adding one.
    const ICONS: [&str; 33] = [
        "add",
        "arrow_back",
        "arrow_upward",
        "bolt",
        "category",
        "celebration",
        "check",
        "checklist",
        "chevron_right",
        "close",
        "cloud_off",
        "coffee",
        "content_copy",
        "edit",
        "expand_less",
        "flag",
        "group",
        "image",
        "local_florist",
        "location_on",
        "music_note",
        "pause",
        "person_add",
        "pets",
        "photo_camera",
        "play_arrow",
        "play_circle",
        "rocket_launch",
        "sailing",
        "sports_soccer",
        "storefront",
        "task_alt",
        "undo",
    ];

    fn sources(dir: &Path, into: &mut String) {
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                sources(&path, into);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                into.push_str(&std::fs::read_to_string(&path).unwrap_or_default());
            }
        }
    }

    #[test]
    fn every_icon_in_the_code_is_in_the_embedded_font() {
        let mut code = String::new();
        sources(Path::new("src"), &mut code);
        let used: BTreeSet<&str> = code
            .split("Icon { name: \"")
            .skip(1)
            .filter_map(|rest| rest.split('"').next())
            .chain(super::PICTURE_ICONS)
            .collect();
        let missing: Vec<_> = used.iter().filter(|name| !ICONS.contains(name)).collect();
        assert!(
            missing.is_empty(),
            "add these to ICONS and the font subset: {missing:?}"
        );
    }
}
