//! What the screens read and what they can do.
//!
//! [`AppState`] is a small handle made only of signals, so it is `Copy`: any
//! click handler can use it without cloning. The device itself sits behind one
//! lock; every method here takes that lock only for the time of one call.
//! Actions report back with a toast, most with an Undo.

use std::sync::atomic::Ordering;

use anyhow::Result;
use chrono::{DateTime, Local, NaiveDate, Utc};
use dioxus::{core::spawn_forever, prelude::*};
use tackly_client::{Device, Geocoder, InviteTicket, Membership, SharedDevice, run_live};
use tackly_protocol::{Family, Picture, PlaceLocation};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::{
    platform,
    settings::{Filter, Settings},
};

/// How long a toast stays (it waits while the stack is touched).
const TOAST_MS: i64 = 4_000;
/// How often the toast timers and the clock tick.
const TICK_MS: u64 = 100;
/// The screens re-read the device at least this often.
const REFRESH_SECONDS: u64 = 20;

/// How to take back what a toast reports.
#[derive(Clone, Debug, PartialEq)]
pub enum Undo {
    DeleteTask(Uuid),
    CancelCompletion(Uuid),
    Assign {
        task: Uuid,
        member: Option<Uuid>,
    },
    DeleteList(Uuid),
    DeleteGroup(Uuid),
    DeletePlace(Uuid),
    RemoveLocation {
        place: Uuid,
        location: Uuid,
    },
    AddLocation {
        place: Uuid,
        location: PlaceLocation,
    },
    ReopenProblem(Uuid),
    ResolveProblem(Uuid),
    /// Back into Upcoming's lists (it was swiped away); nothing on the device.
    Undismiss(String),
}

/// Upcoming while it is open: the focus being planned (not saved yet), what
/// was swiped away this round, and the problem stepped into.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Upcoming {
    pub focus: Vec<Uuid>,
    pub dismissed: Vec<String>,
    pub problem: Option<Uuid>,
    /// The day the draft was started for (it starts from that day's plan).
    pub day: Option<NaiveDate>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Toast {
    pub id: u64,
    pub text: String,
    pub undo: Option<Undo>,
    left_ms: i64,
}

#[derive(Clone, Copy, PartialEq)]
pub struct AppState {
    device: Signal<SharedDevice>,
    pub geocoder: Signal<Geocoder>,
    pub family: Signal<Family>,
    pub membership: Signal<Option<Membership>>,
    /// This phone's own ID, to tell "you" from the others.
    pub my_id: Signal<Uuid>,
    /// Whether the live connection to the relay is up.
    pub online: Signal<bool>,
    pub now: Signal<DateTime<Utc>>,
    pub settings: Signal<Settings>,
    /// Newest last.
    pub toasts: Signal<Vec<Toast>>,
    /// While the toast stack is touched it fans out and its timers wait.
    pub toasts_fanned: Signal<bool>,
    next_toast: Signal<u64>,
    sync_started: Signal<bool>,
    pub upcoming: Signal<Upcoming>,
    /// "Plan later": Upcoming waits until then.
    pub plan_snoozed_until: Signal<Option<DateTime<Local>>>,
    /// All of today's priorities are done: which kitten celebrates.
    pub celebrate: Signal<Option<usize>>,
}

impl AppState {
    /// Opens this phone's data. Call once, inside a component.
    pub fn open() -> Result<Self> {
        let device = Device::open(platform::data_dir())?;
        Ok(Self {
            family: Signal::new(device.state()?),
            membership: Signal::new(device.membership().cloned()),
            my_id: Signal::new(device.device_id()),
            device: Signal::new(std::sync::Arc::new(Mutex::new(device))),
            geocoder: Signal::new(Geocoder::from_env()?),
            online: Signal::new(false),
            now: Signal::new(Utc::now()),
            settings: Signal::new(Settings::load()),
            toasts: Signal::new(Vec::new()),
            toasts_fanned: Signal::new(false),
            next_toast: Signal::new(0),
            sync_started: Signal::new(false),
            upcoming: Signal::new(Upcoming::default()),
            plan_snoozed_until: Signal::new(None),
            celebrate: Signal::new(None),
        })
    }

    pub fn device(&self) -> SharedDevice {
        self.device.peek().clone()
    }

    /// Reads the device into the signals. Cheap enough to call after any change.
    pub async fn refresh(self) {
        let (family, membership, online, my_id) = {
            let device = self.device().lock_owned().await;
            (
                device.state(),
                device.membership().cloned(),
                device.online_flag().load(Ordering::Relaxed),
                device.device_id(),
            )
        };
        let (mut family_signal, mut membership_signal) = (self.family, self.membership);
        let (mut my_id_signal, mut online_signal) = (self.my_id, self.online);
        if let Ok(family) = family
            && *family_signal.peek() != family
        {
            family_signal.set(family);
        }
        membership_signal.set(membership);
        my_id_signal.set(my_id);
        online_signal.set(online);
    }

    // ---- toasts ------------------------------------------------------------------------------

    /// Shows a toast, with an Undo when given one.
    pub fn toast(self, text: impl Into<String>, undo: Option<Undo>) {
        let (mut toasts, mut next) = (self.toasts, self.next_toast);
        let id = next() + 1;
        next.set(id);
        toasts.write().push(Toast {
            id,
            text: text.into(),
            undo,
            left_ms: TOAST_MS,
        });
    }

    pub fn say(self, text: impl Into<String>) {
        self.toast(text, None);
    }

    pub fn undo(self, toast_id: u64, undo: Undo) {
        let mut toasts = self.toasts;
        toasts.write().retain(|toast| toast.id != toast_id);
        if let Undo::Undismiss(reference) = undo {
            let mut upcoming = self.upcoming;
            upcoming
                .write()
                .dismissed
                .retain(|known| *known != reference);
            return;
        }
        self.act(move |device| async move {
            let mut device = device.lock().await;
            match undo {
                Undo::DeleteTask(task) => device.delete_task(task).await,
                Undo::CancelCompletion(task) => device.undo_completion(task).await,
                Undo::Assign { task, member } => device.assign_task(task, member).await,
                Undo::DeleteList(list) => device.delete_list(list).await,
                Undo::DeleteGroup(group) => device.delete_place_group(group).await,
                Undo::DeletePlace(place) => device.delete_place(place).await,
                Undo::RemoveLocation { place, location } => {
                    device.remove_place_location(place, location).await
                }
                Undo::AddLocation { place, location } => {
                    device.add_place_location(place, location).await
                }
                Undo::ReopenProblem(problem) => device.reopen_problem(problem).await,
                Undo::ResolveProblem(problem) => device.resolve_problem(problem).await,
                Undo::Undismiss(_) => Ok(()),
            }
        });
    }

    /// Runs what a person asked for, then refreshes. A failure becomes a
    /// toast. The work belongs to the app, not to the button's component:
    /// sheets close right after the click, and a closed component's work is cancelled.
    fn act<Fut>(self, action: impl FnOnce(SharedDevice) -> Fut + 'static)
    where
        Fut: std::future::Future<Output = Result<()>> + 'static,
    {
        spawn_forever(async move {
            if let Err(error) = action(self.device()).await {
                self.say(format!("{error:#}"));
            }
            self.refresh().await;
        });
    }

    /// Like [`Self::act`], for actions that report with a toast.
    fn act_then<Fut>(self, action: impl FnOnce(SharedDevice) -> Fut + 'static)
    where
        Fut: std::future::Future<Output = Result<(String, Option<Undo>)>> + 'static,
    {
        spawn_forever(async move {
            match action(self.device()).await {
                Ok((text, undo)) => self.toast(text, undo),
                Err(error) => self.say(format!("{error:#}")),
            }
            self.refresh().await;
        });
    }

    fn name_of(self, member: Uuid) -> String {
        if member == *self.my_id.peek() {
            "you".into()
        } else {
            self.family.peek().member_name(member).to_owned()
        }
    }

    fn title_of(self, task: Uuid) -> String {
        self.family
            .peek()
            .tasks
            .get(&task)
            .map(|task| task.title.clone())
            .unwrap_or_default()
    }

    // ---- tasks ---------------------------------------------------------------------------------

    /// Adds a task to a list (`None`: "Other"), needed at the given places.
    pub fn add_task(self, title: String, list: Option<Uuid>, places: Vec<Uuid>) {
        self.act_then(move |device| async move {
            let task = device.lock().await.add_task(&title, list, &places).await?;
            Ok((
                format!("Added {}", title.trim()),
                Some(Undo::DeleteTask(task)),
            ))
        });
    }

    /// Finishes a task at once; where the phone is follows when the GPS
    /// knows (a fix takes seconds). Undone by then: nothing to note. The last
    /// of today's priorities: a kitten celebrates.
    pub fn complete_task(self, task: Uuid) {
        let title = self.title_of(task);
        let (me, today) = (*self.my_id.peek(), platform::today());
        let mut celebrate = self.celebrate;
        self.act_then(move |device| async move {
            let family = {
                let mut device = device.lock().await;
                device.complete_task(task, None).await?;
                device.state()?
            };
            let planned = family
                .plan(me, today)
                .is_some_and(|plan| plan.contains(&task));
            if planned && family.focus(me, today).is_empty() {
                let kitten = platform::local_now().timestamp_subsec_millis() as usize % 4;
                celebrate.set(Some(kitten));
            }
            spawn_forever(async move {
                if let Some(location) = platform::location().await {
                    let _ = device.lock().await.locate_completion(task, location).await;
                }
            });
            Ok((format!("{title} done"), Some(Undo::CancelCompletion(task))))
        });
    }

    pub fn start_task(self, task: Uuid) {
        self.act(move |device| async move { device.lock().await.start_task(task).await });
    }

    pub fn pause_task(self, task: Uuid) {
        self.act(move |device| async move { device.lock().await.pause_task(task).await });
    }

    /// Gives a task to someone (or takes it yourself).
    pub fn assign_task(self, task: Uuid, member: Uuid) {
        let (title, me) = (self.title_of(task), *self.my_id.peek());
        let text = if member == me {
            format!("You took {title}")
        } else {
            format!("{title} is now {}’s", self.name_of(member))
        };
        self.act_then(move |device| async move {
            device.lock().await.assign_task(task, Some(member)).await?;
            Ok((text, Some(Undo::Assign { task, member: None })))
        });
    }

    // ---- problems ------------------------------------------------------------------------------

    fn problem_title(self, problem: Uuid) -> String {
        self.family
            .peek()
            .problems
            .get(&problem)
            .map(|problem| problem.title.clone())
            .unwrap_or_default()
    }

    /// Notices a problem for someone (yourself or another member). `known`: a
    /// problem the family has; otherwise it is made first from `title`, `icon`
    /// and its solutions.
    pub fn notice_problem(
        self,
        known: Option<Uuid>,
        title: String,
        icon: String,
        solutions: Vec<String>,
        member: Uuid,
    ) {
        let raised = member != *self.my_id.peek();
        let name = self.name_of(member);
        self.act_then(move |device| async move {
            let mut device = device.lock().await;
            let problem = match known {
                Some(problem) => problem,
                None => {
                    let solutions: Vec<&str> = solutions.iter().map(String::as_str).collect();
                    device.create_problem(&title, &icon, &solutions).await?
                }
            };
            device.notice_problem(problem, member).await?;
            let text = if raised {
                format!("Raised for {name}: {}", title.trim())
            } else {
                format!("Noticed: {}", title.trim())
            };
            Ok((text, Some(Undo::ResolveProblem(problem))))
        });
    }

    /// A new problem, without solutions yet; `then` gets it (to edit it).
    pub fn create_problem(self, title: String, then: impl FnOnce(Uuid) + 'static) {
        spawn_forever(async move {
            let made = self
                .device()
                .lock()
                .await
                .create_problem(&title, "report", &[])
                .await;
            match made {
                Ok(problem) => {
                    self.refresh().await;
                    then(problem);
                }
                Err(error) => self.say(format!("{error:#}")),
            }
        });
    }

    pub fn rename_problem(self, problem: Uuid, title: String) {
        self.act(
            move |device| async move { device.lock().await.rename_problem(problem, &title).await },
        );
    }

    pub fn add_solution(self, problem: Uuid, title: String) {
        self.act(
            move |device| async move { device.lock().await.add_solution(problem, &title).await },
        );
    }

    pub fn remove_solution(self, problem: Uuid, solution: Uuid) {
        self.act(move |device| async move {
            device.lock().await.remove_solution(problem, solution).await
        });
    }

    pub fn assign_problem(self, problem: Uuid, member: Uuid) {
        let (title, name) = (self.problem_title(problem), self.name_of(member));
        let text = if member == *self.my_id.peek() {
            format!("{title}: yours now")
        } else {
            format!("{title}: {name} has it now")
        };
        self.act_then(move |device| async move {
            device.lock().await.assign_problem(problem, member).await?;
            Ok((text, None))
        });
    }

    pub fn resolve_problem(self, problem: Uuid) {
        let title = self.problem_title(problem);
        self.act_then(move |device| async move {
            device.lock().await.resolve_problem(problem).await?;
            Ok((
                format!("Resolved: {title}"),
                Some(Undo::ReopenProblem(problem)),
            ))
        });
    }

    // ---- plans ---------------------------------------------------------------------------------

    /// My focus for a day, in order (free tasks become mine).
    pub fn plan_day(self, day: NaiveDate, tasks: Vec<Uuid>, done: Option<String>) {
        self.act(move |device| async move {
            device.lock().await.plan_day(day, &tasks).await?;
            if let Some(text) = done {
                self.say(text);
            }
            Ok(())
        });
    }

    pub fn set_plan_at(self, time: Option<String>) {
        let mut settings = self.settings;
        settings.write().plan_at = time;
        settings.peek().save();
    }

    // ---- lists ---------------------------------------------------------------------------------

    pub fn create_list(self, name: String) {
        self.act_then(move |device| async move {
            let list = device.lock().await.create_list(&name).await?;
            Ok((
                format!("Added {}", name.trim()),
                Some(Undo::DeleteList(list)),
            ))
        });
    }

    pub fn rename_list(self, list: Uuid, name: String) {
        self.act(move |device| async move { device.lock().await.rename_list(list, &name).await });
    }

    pub fn delete_list(self, list: Uuid) {
        self.act(move |device| async move { device.lock().await.delete_list(list).await });
    }

    // ---- places -------------------------------------------------------------------------------

    pub fn create_group(self, name: String) {
        self.act_then(move |device| async move {
            let group = device.lock().await.create_place_group(&name).await?;
            Ok((
                format!("Added {}", name.trim()),
                Some(Undo::DeleteGroup(group)),
            ))
        });
    }

    pub fn rename_group(self, group: Uuid, name: String) {
        self.act(move |device| async move {
            device.lock().await.rename_place_group(group, &name).await
        });
    }

    pub fn delete_group(self, group: Uuid) {
        self.act(move |device| async move { device.lock().await.delete_place_group(group).await });
    }

    /// A new group from Edit place: the place moves into it.
    pub fn create_group_for(self, place: Uuid, name: String) {
        self.act(move |device| async move {
            let mut device = device.lock().await;
            let group = device.create_place_group(&name).await?;
            device.move_place(place, group).await
        });
    }

    pub fn create_place(self, group: Uuid, name: String, location: PlaceLocation) {
        self.act_then(move |device| async move {
            let place = device
                .lock()
                .await
                .create_place(group, &name, location)
                .await?;
            Ok((
                format!("Added {}", name.trim()),
                Some(Undo::DeletePlace(place)),
            ))
        });
    }

    pub fn rename_place(self, place: Uuid, name: String) {
        self.act(move |device| async move { device.lock().await.rename_place(place, &name).await });
    }

    pub fn move_place(self, place: Uuid, group: Uuid) {
        self.act(move |device| async move { device.lock().await.move_place(place, group).await });
    }

    pub fn add_location(self, place: Uuid, location: PlaceLocation) {
        let (name, id) = (location.name.clone(), location.id);
        self.act_then(move |device| async move {
            device
                .lock()
                .await
                .add_place_location(place, location)
                .await?;
            Ok((
                format!("Added {name}"),
                Some(Undo::RemoveLocation {
                    place,
                    location: id,
                }),
            ))
        });
    }

    pub fn remove_location(self, place: Uuid, location: PlaceLocation) {
        self.act_then(move |device| async move {
            device
                .lock()
                .await
                .remove_place_location(place, location.id)
                .await?;
            Ok((
                format!("Removed {}", location.name),
                Some(Undo::AddLocation { place, location }),
            ))
        });
    }

    // ---- me and the family -------------------------------------------------------------

    pub fn rename_me(self, name: String) {
        self.act(move |device| async move { device.lock().await.rename_me(&name).await });
    }

    pub fn set_picture(self, picture: Picture) {
        self.act(move |device| async move { device.lock().await.set_picture(picture).await });
    }

    pub fn approve_join(self, ticket: InviteTicket, joining_device: Uuid) {
        self.act(move |device| async move {
            device
                .lock()
                .await
                .approve_join(&ticket, joining_device)
                .await
        });
    }

    pub fn leave_family(self) {
        self.act(move |device| async move { device.lock().await.logout() });
    }

    // ---- this phone's settings ----------------------------------------------------------

    pub fn set_filter(self, filter: Filter) {
        let mut settings = self.settings;
        settings.write().filter = filter;
        settings.peek().save();
    }

    pub fn set_scheme(self, scheme: &str) {
        let mut settings = self.settings;
        settings.write().scheme = scheme.to_owned();
        settings.peek().save();
    }

    // ---- live sync and the clock ------------------------------------------------------------

    /// Starts keeping this phone in sync, once: whatever arrives from the relay
    /// refreshes the screens. Also runs the toast timers and the clock.
    pub fn start_sync(self) {
        let mut started = self.sync_started;
        if started.replace(true) {
            return;
        }
        let (changed, mut wake) = tokio::sync::mpsc::unbounded_channel::<()>();
        let device = self.device();
        spawn(async move {
            run_live(device, move || {
                // The receiver lives as long as the app does.
                let _ = changed.send(());
            })
            .await;
        });
        spawn(async move {
            let periodic = std::time::Duration::from_secs(REFRESH_SECONDS);
            loop {
                tokio::select! {
                    update = wake.recv() => if update.is_none() { break },
                    () = tokio::time::sleep(periodic) => {}
                }
                self.refresh().await;
            }
        });
        spawn(async move {
            let (mut toasts, mut now) = (self.toasts, self.now);
            let mut ticks = 0_u32;
            loop {
                tokio::time::sleep(std::time::Duration::from_millis(TICK_MS)).await;
                if !*self.toasts_fanned.peek() && !toasts.peek().is_empty() {
                    let mut list = toasts.write();
                    for toast in list.iter_mut() {
                        toast.left_ms -= TICK_MS as i64;
                    }
                    list.retain(|toast| toast.left_ms > 0);
                }
                ticks += 1;
                if ticks.is_multiple_of(10) {
                    now.set(Utc::now());
                }
            }
        });
    }
}
