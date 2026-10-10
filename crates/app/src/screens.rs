use chrono::{DateTime, Utc};
use dioxus::prelude::*;
use tackly_client::{InviteProgress, InviteTicket, JoinRequest};
use tackly_protocol::{Activity, ActivityKind, Task, TaskStatus};
use uuid::Uuid;

use crate::{platform, state::AppState};

const EMOJIS: [&str; 10] = ["✅", "🍽", "🧺", "🗑", "🛒", "🧹", "🪴", "🐕", "🚗", "📞"];

fn ago(now: DateTime<Utc>, at: DateTime<Utc>) -> String {
    let secs = (now - at).num_seconds().max(0);
    match secs {
        0..=59 => "just now".into(),
        60..=3599 => format!("{} min ago", secs / 60),
        3600..=86399 => format!("{} h ago", secs / 3600),
        _ => format!("{} d ago", secs / 86400),
    }
}

fn duration(secs: i64) -> String {
    if secs < 60 {
        format!("{secs} s")
    } else {
        format!("{} min {} s", secs / 60, secs % 60)
    }
}

fn initial(name: &str) -> String {
    name.chars()
        .next()
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_default()
}

#[component]
pub fn Snackbar() -> Element {
    let mut state = use_context::<AppState>();
    let Some(message) = (state.toast)() else {
        return rsx! {};
    };
    rsx! {
        div { class: "snack",
            span { "{message}" }
            button { onclick: move |_| state.toast.set(None), "OK" }
        }
    }
}

// ---------------------------------------------------------------- welcome

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Choose,
    Create,
    Join,
}

#[component]
pub fn Onboarding() -> Element {
    let state = use_context::<AppState>();
    let mut mode = use_signal(|| Mode::Choose);
    let mut server = use_signal(platform::default_server);
    let mut family_name = use_signal(String::new);
    let mut my_name = use_signal(String::new);
    let mut code = use_signal(String::new);
    let mut request = use_signal(|| None::<JoinRequest>);
    let mut busy = use_signal(|| false);

    let create = {
        let state = state.clone();
        move |_| {
            let state = state.clone();
            busy.set(true);
            spawn(async move {
                let result = state
                    .device
                    .lock()
                    .await
                    .create_family(&server(), &family_name(), &my_name())
                    .await;
                if let Err(error) = result {
                    state.clone().say(format!("{error:#}"));
                }
                busy.set(false);
                state.refresh().await;
            });
        }
    };
    let ask = {
        let state = state.clone();
        move |_| {
            let state = state.clone();
            busy.set(true);
            spawn(async move {
                let asked = state.device.lock().await.request_join(&code()).await;
                busy.set(false);
                let join = match asked {
                    Ok(join) => join,
                    Err(error) => return state.say(format!("{error:#}")),
                };
                request.set(Some(join.clone()));
                // Poll until the owner approves (or the code is cancelled).
                loop {
                    if request.peek().is_none() {
                        return;
                    }
                    let done = state
                        .device
                        .lock()
                        .await
                        .complete_join(&join, &my_name())
                        .await;
                    match done {
                        Ok(true) => break,
                        Ok(false) => tokio::time::sleep(std::time::Duration::from_secs(1)).await,
                        Err(error) => {
                            request.set(None);
                            return state.say(format!("{error:#}"));
                        }
                    }
                }
                request.set(None);
                state.refresh().await;
            });
        }
    };

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
                    div { class: "field", label { "Your name" }
                        input { value: "{my_name}", oninput: move |e| my_name.set(e.value()) } }
                    div { class: "field", label { "Family name" }
                        input { value: "{family_name}", oninput: move |e| family_name.set(e.value()) } }
                    div { class: "field", label { "Server" }
                        input { value: "{server}", oninput: move |e| server.set(e.value()) } }
                    p { class: "meta", "No server right now? Tasks work anyway and sync when it is reachable." }
                    div { class: "row",
                        button { class: "btn text", onclick: move |_| mode.set(Mode::Choose), "Back" }
                        button { class: "btn", disabled: busy(), onclick: create, "Create" }
                    }
                },
                Mode::Join => rsx! {
                    if let Some(join) = request() {
                        p { "Compare this code with the owner's phone, then wait for approval." }
                        div { class: "code", "{join.confirmation}" }
                        div { class: "row",
                            button { class: "btn text", onclick: move |_| request.set(None), "Cancel" }
                        }
                    } else {
                        div { class: "field", label { "Your name" }
                            input { value: "{my_name}", oninput: move |e| my_name.set(e.value()) } }
                        div { class: "field", label { "Invitation code" }
                            textarea { value: "{code}", oninput: move |e| code.set(e.value()) } }
                        div { class: "row",
                            button { class: "btn text", onclick: move |_| mode.set(Mode::Choose), "Back" }
                            button { class: "btn", disabled: busy(), onclick: ask, "Ask to join" }
                        }
                    }
                },
            }
        }
    }
}

// ------------------------------------------------------------------- home

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Tasks,
    Activity,
    Family,
}

#[derive(Clone, PartialEq)]
enum Sheet {
    None,
    AddTask,
    Finish(Uuid),
}

#[component]
pub fn Home() -> Element {
    let state = use_context::<AppState>();
    let mut tab = use_signal(|| Tab::Tasks);
    let mut sheet = use_signal(|| Sheet::None);
    let family = (state.family)();
    let title = family.name.clone().unwrap_or_else(|| "Tackly".into());
    let online = (state.online)();
    let unsynced = (state.membership)().is_some_and(|m| !m.registered());
    let nav = |target: Tab, icon: &'static str, label: &'static str| {
        let class = if tab() == target { "active" } else { "" };
        rsx! {
            button { class: "{class}", onclick: move |_| tab.set(target),
                div { class: "pill", "{icon}" }
                "{label}"
            }
        }
    };
    rsx! {
        div { class: "app-bar",
            div { style: "flex:1",
                h1 { "Tackly 🌱" }
                div { class: "sub", "{title} · {family.members.len()} members" }
            }
            if online {
                span { class: "sync on", i {} "Live" }
            } else {
                span { class: "sync off", i {} if unsynced { "Offline · not shared yet" } else { "Offline · will sync" } }
            }
        }
        div { class: "content",
            match tab() {
                Tab::Tasks => rsx! { TasksTab { open_finish: move |id| sheet.set(Sheet::Finish(id)) } },
                Tab::Activity => rsx! { ActivityTab {} },
                Tab::Family => rsx! { FamilyTab {} },
            }
        }
        if tab() == Tab::Tasks {
            button { class: "fab", onclick: move |_| sheet.set(Sheet::AddTask), span { "+" } "New task" }
        }
        div { class: "nav",
            {nav(Tab::Tasks, "✓", "Tasks")}
            {nav(Tab::Activity, "🕘", "Activity")}
            {nav(Tab::Family, "👪", "Family")}
        }
        match sheet() {
            Sheet::None => rsx! {},
            Sheet::AddTask => rsx! { AddTaskSheet { close: move |_| sheet.set(Sheet::None) } },
            Sheet::Finish(id) => rsx! { FinishSheet { task_id: id, close: move |_| sheet.set(Sheet::None) } },
        }
    }
}

#[component]
fn TasksTab(open_finish: EventHandler<Uuid>) -> Element {
    let state = use_context::<AppState>();
    let family = (state.family)();
    let conflicts: Vec<&Task> = family.tasks.values().filter(|t| t.has_conflict()).collect();
    let mut todo: Vec<&Task> = family.tasks.values().filter(|t| !t.is_done()).collect();
    todo.sort_by_key(|t| !matches!(t.status, TaskStatus::InProgress { .. }));
    let mut done: Vec<&Task> = family
        .tasks
        .values()
        .filter(|t| t.is_done() && !t.has_conflict())
        .collect();
    done.sort_by_key(|t| match &t.status {
        TaskStatus::Done { at, .. } => std::cmp::Reverse(*at),
        _ => std::cmp::Reverse(DateTime::<Utc>::MIN_UTC),
    });
    if family.tasks.is_empty() {
        return rsx! { div { class: "empty", div { class: "big", "🌱" } p { "No tasks yet. Add the first one." } } };
    }
    rsx! {
        for task in conflicts {
            ConflictCard { key: "{task.id}", task: task.clone() }
        }
        if !todo.is_empty() {
            div { class: "section", "To do" }
        }
        for task in todo {
            TaskCard { key: "{task.id}", task: task.clone(), open_finish }
        }
        if !done.is_empty() {
            div { class: "section", "Done" }
        }
        for task in done {
            TaskCard { key: "{task.id}", task: task.clone(), open_finish }
        }
    }
}

#[component]
fn TaskCard(task: Task, open_finish: EventHandler<Uuid>) -> Element {
    let state = use_context::<AppState>();
    let family = (state.family)();
    let (now, me) = ((state.now)(), (state.my_id)());
    let id = task.id;
    let (class, details) = match &task.status {
        TaskStatus::Open => ("card", rsx! {}),
        TaskStatus::InProgress { by, since, .. } => {
            let who = if *by == me {
                "You are".to_owned()
            } else {
                format!("{} is", family.member_name(*by))
            };
            (
                "card doing",
                rsx! { div { class: "meta", "{who} on it · started {ago(now, *since)}" } },
            )
        }
        TaskStatus::Done {
            by, at, metadata, ..
        } => {
            let who = if *by == me {
                "You".to_owned()
            } else {
                family.member_name(*by).to_owned()
            };
            let took = metadata.duration_seconds.map(duration);
            let place = metadata
                .location
                .map(|g| format!("📍 {:.4}, {:.4}", g.latitude, g.longitude));
            let note = metadata.note.clone();
            (
                "card done",
                rsx! {
                    div { class: "meta", "Done by {who} · {ago(now, *at)}" }
                    div { class: "chips",
                        if let Some(took) = took { span { class: "chip ok", "⏱ {took}" } }
                        if let Some(place) = place { span { class: "chip", "{place}" } }
                        if let Some(note) = note { span { class: "chip", "💬 {note}" } }
                    }
                },
            )
        }
    };
    let done = task.is_done();
    let starter = matches!(task.status, TaskStatus::Open)
        || matches!(&task.status, TaskStatus::InProgress { by, .. } if *by != me);
    rsx! {
        div { class: "{class}",
            div { class: "emoji", "{task.emoji}" }
            div { class: "body",
                div { class: "title", "{task.title}" }
                {details}
                div { class: "actions",
                    if !done && starter {
                        button { class: "btn tonal", onclick: {
                            let state = state.clone();
                            move |_| { let s = state.clone(); state.clone().run(async move { s.device.lock().await.start_task(id).await }) }
                        }, "Start" }
                    }
                    if !done {
                        button { class: "btn", onclick: move |_| open_finish.call(id), "Finish" }
                    } else {
                        button { class: "btn text", onclick: {
                            let state = state.clone();
                            move |_| { let s = state.clone(); state.clone().run(async move { s.device.lock().await.reopen_task(id).await }) }
                        }, "Reopen" }
                    }
                }
            }
        }
    }
}

#[component]
fn ConflictCard(task: Task) -> Element {
    let state = use_context::<AppState>();
    let family = (state.family)();
    let me = (state.my_id)();
    let id = task.id;
    let involved = task.claims.iter().any(|c| c.by == me);
    let names: Vec<String> = task
        .claims
        .iter()
        .map(|c| family.member_name(c.by).to_owned())
        .collect();
    let names = names.join(" and ");
    rsx! {
        div { class: "card conflict",
            div { class: "title", "{task.emoji} {task.title}: finished twice" }
            div { class: "meta", "{names} both finished this task." }
            if involved {
                div { class: "actions",
                    for claim in task.claims.clone() {
                        button { key: "{claim.completion_event_id}", class: "btn tonal", onclick: {
                            let state = state.clone();
                            move |_| {
                                let s = state.clone();
                                let keep = claim.completion_event_id;
                                state.clone().run(async move { s.device.lock().await.resolve_conflict(id, keep).await })
                            }
                        }, "Keep {family.member_name(claim.by)}'s" }
                    }
                }
            } else {
                div { class: "meta", "Waiting for {names} to decide." }
            }
        }
    }
}

#[component]
fn AddTaskSheet(close: EventHandler<()>) -> Element {
    let state = use_context::<AppState>();
    let mut title = use_signal(String::new);
    let mut emoji = use_signal(|| EMOJIS[0].to_owned());
    rsx! {
        div { class: "scrim", onclick: move |_| close.call(()),
            div { class: "sheet", onclick: move |e| e.stop_propagation(),
                div { class: "handle" }
                h2 { "New task" }
                div { class: "field", label { "What needs doing?" }
                    input { value: "{title}", autofocus: true, oninput: move |e| title.set(e.value()) } }
                div { class: "emoji-row",
                    for e in EMOJIS {
                        button { key: "{e}", class: if emoji() == e { "sel" } else { "" }, onclick: move |_| emoji.set(e.to_owned()), "{e}" }
                    }
                }
                div { class: "row",
                    button { class: "btn text", onclick: move |_| close.call(()), "Cancel" }
                    button { class: "btn", disabled: title().trim().is_empty(), onclick: move |_| {
                        let s = state.clone();
                        let (t, e) = (title(), emoji());
                        state.clone().run(async move { s.device.lock().await.add_task(&t, &e).await });
                        close.call(());
                    }, "Add" }
                }
            }
        }
    }
}

#[component]
fn FinishSheet(task_id: Uuid, close: EventHandler<()>) -> Element {
    let state = use_context::<AppState>();
    let mut note = use_signal(String::new);
    let mut with_location = use_signal(|| true);
    let location = platform::location();
    let title = (state.family)()
        .tasks
        .get(&task_id)
        .map(|t| t.title.clone())
        .unwrap_or_default();
    rsx! {
        div { class: "scrim", onclick: move |_| close.call(()),
            div { class: "sheet", onclick: move |e| e.stop_propagation(),
                div { class: "handle" }
                h2 { "Finish “{title}”" }
                div { class: "field", label { "Note (optional)" }
                    input { value: "{note}", oninput: move |e| note.set(e.value()) } }
                if location.is_some() {
                    label { class: "switch",
                        input { r#type: "checkbox", checked: with_location(), onchange: move |e| with_location.set(e.checked()) }
                        "Add where I am"
                    }
                }
                div { class: "row",
                    button { class: "btn text", onclick: move |_| close.call(()), "Cancel" }
                    button { class: "btn", onclick: move |_| {
                        let s = state.clone();
                        let (n, place) = (note(), if with_location() { platform::location() } else { None });
                        state.clone().run(async move { s.device.lock().await.complete_task(task_id, Some(n), place).await });
                        close.call(());
                    }, "Done" }
                }
            }
        }
    }
}

// --------------------------------------------------------------- activity

fn describe(activity: &Activity, who: &str) -> (&'static str, String) {
    let s = &activity.subject;
    match activity.kind {
        ActivityKind::FamilyCreated => ("🌱", format!("{who} created {s}")),
        ActivityKind::Joined => ("👋", format!("{s} joined the family")),
        ActivityKind::Created => ("➕", format!("{who} added “{s}”")),
        ActivityKind::Started => ("▶️", format!("{who} started “{s}”")),
        ActivityKind::Finished => ("✅", format!("{who} finished “{s}”")),
        ActivityKind::Reopened => ("↩️", format!("{who} reopened “{s}”")),
        ActivityKind::Conflicted => ("⚠️", format!("{who} also finished “{s}”")),
        ActivityKind::Resolved => ("🤝", format!("{who} settled “{s}”")),
    }
}

#[component]
fn ActivityTab() -> Element {
    let state = use_context::<AppState>();
    let family = (state.family)();
    let now = (state.now)();
    rsx! {
        for activity in family.activity.iter().rev() {
            {
                let (icon, text) = describe(activity, family.member_name(activity.by));
                rsx! {
                    div { key: "{activity.event_id}", class: "card",
                        div { class: "emoji", "{icon}" }
                        div { class: "body",
                            div { class: "title", style: "font-size:15px", "{text}" }
                            div { class: "meta", "{ago(now, activity.at)}" }
                        }
                    }
                }
            }
        }
    }
}

// ----------------------------------------------------------------- family

#[component]
fn FamilyTab() -> Element {
    let state = use_context::<AppState>();
    let family = (state.family)();
    let me = (state.my_id)();
    let owner = (state.membership)().is_some_and(|m| m.owner);
    let mut ticket = use_signal(|| None::<InviteTicket>);
    let mut progress = use_signal(|| InviteProgress::Waiting);
    let mut confirm_logout = use_signal(|| false);

    let invite = {
        let state = state.clone();
        move |_| {
            let state = state.clone();
            spawn(async move {
                let made = state.device.lock().await.create_invite().await;
                let new = match made {
                    Ok(new) => new,
                    Err(error) => {
                        eprintln!("invite: could not create: {error:#}");
                        return state.say(format!("Sharing needs the server: {error:#}"));
                    }
                };
                let id = new.invite_id;
                progress.set(InviteProgress::Waiting);
                ticket.set(Some(new.clone()));
                loop {
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    if ticket.peek().as_ref().map(|t| t.invite_id) != Some(id) {
                        return;
                    }
                    // Not in the `match` head: its temporary guard would live
                    // through the arms, and `refresh` locks the device again.
                    let progress_now = state.device.lock().await.invite_progress(&new).await;
                    match progress_now {
                        Ok(InviteProgress::Approved | InviteProgress::Gone) => {
                            ticket.set(None);
                            state.clone().refresh().await;
                            return;
                        }
                        Ok(next) => progress.set(next),
                        Err(_) => {}
                    }
                }
            });
        }
    };

    rsx! {
        div { class: "section", "Members" }
        for member in family.members.values() {
            div { key: "{member.device_id}", class: "card", style: "align-items:center",
                div { class: "avatar", "{initial(&member.name)}" }
                div { class: "body",
                    div { class: "title", "{member.name}" if member.device_id == me { " (you)" } }
                    div { class: "meta", if member.owner { "Head of the family" } else { "Member" } }
                }
            }
        }
        if owner {
            div { class: "section", "Invite" }
            if let Some(t) = ticket() {
                div { class: "card", style: "flex-direction:column",
                    div { class: "meta", "Send this code to the other phone. It expires in 5 minutes." }
                    div { class: "field", textarea { readonly: true, value: "{t.code}" } }
                    match progress() {
                        InviteProgress::Requested { device_id, confirmation } => rsx! {
                            div { class: "meta", "Someone asked to join. Do both phones show this code?" }
                            div { class: "code", "{confirmation}" }
                            div { class: "actions",
                                button { class: "btn", onclick: {
                                    let (state, t) = (state.clone(), t.clone());
                                    move |_| {
                                        let (s, t) = (state.clone(), t.clone());
                                        state.clone().run(async move { s.device.lock().await.approve_join(&t, device_id).await });
                                    }
                                }, "Yes, let them in" }
                                button { class: "btn text", onclick: move |_| ticket.set(None), "No" }
                            }
                        },
                        _ => rsx! { div { class: "meta", "Waiting for the other phone…" } },
                    }
                    div { class: "actions", button { class: "btn text", onclick: move |_| ticket.set(None), "Close" } }
                }
            } else {
                button { class: "btn tonal", onclick: invite, "Invite someone" }
            }
        }
        div { class: "section", "This phone" }
        button { class: "btn danger", onclick: move |_| confirm_logout.set(true), "Leave family on this phone" }
        if confirm_logout() {
            div { class: "scrim", onclick: move |_| confirm_logout.set(false),
                div { class: "dialog", onclick: move |e| e.stop_propagation(),
                    h2 { "Leave this family?" }
                    p { "This erases the family's tasks from this phone. The others keep theirs." }
                    div { class: "row",
                        button { class: "btn text", onclick: move |_| confirm_logout.set(false), "Cancel" }
                        button { class: "btn danger", onclick: {
                            let state = state.clone();
                            move |_| {
                                let s = state.clone();
                                confirm_logout.set(false);
                                state.clone().run(async move { s.device.lock().await.logout() });
                            }
                        }, "Leave" }
                    }
                }
            }
        }
    }
}
