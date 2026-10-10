//! End-to-end: a real relay (a SQLite file, real HTTP and SSE) and one real
//! `Device` per phone, each with its own database and secrets. Windows and
//! clicking come later, in the acceptance suite; this one drives the device
//! core directly and is fast.

use std::{
    future::Future,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail, ensure};
use tackly_client::{Device, InviteProgress, SharedDevice, api::Api, run_live};
use tackly_protocol::{Family, GeoPoint, TaskStatus};
use tackly_testkit::Relay;
use tokio::{sync::Mutex, task::JoinHandle};
use uuid::Uuid;

// ---- harness ---------------------------------------------------------------

/// Polls `check` until it holds, or fails after twenty seconds.
async fn eventually<F: Future<Output = Result<bool>>>(
    what: &str,
    mut check: impl FnMut() -> F,
) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        if check().await? {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    bail!("timed out waiting for: {what}")
}

/// One phone: a device, and the sync loop the app would run for it.
struct Phone {
    device: SharedDevice,
    live: Option<JoinHandle<()>>,
}

impl Drop for Phone {
    fn drop(&mut self) {
        if let Some(live) = &self.live {
            live.abort();
        }
    }
}

impl Phone {
    fn new(name: &str, dir: &Path) -> Result<Self> {
        Ok(Self {
            device: Arc::new(Mutex::new(Device::open(dir.join(name))?)),
            live: None,
        })
    }

    /// Same as the app: sync keeps running while the phone is in a family.
    fn go_live(&mut self) {
        self.live = Some(tokio::spawn(run_live(self.device.clone(), || {})));
    }

    async fn state(&self) -> Result<Family> {
        self.device.lock().await.state()
    }

    async fn id(&self) -> Uuid {
        self.device.lock().await.device_id()
    }

    // What a person does. It succeeds locally whatever the network does; the
    // device's upload trigger syncs it as soon as the relay is reachable.

    async fn add_task(&self, title: &str, emoji: &str) -> Result<Uuid> {
        self.device.lock().await.add_task(title, emoji).await
    }

    async fn start(&self, task: Uuid) -> Result<()> {
        self.device.lock().await.start_task(task).await
    }

    async fn complete(
        &self,
        task: Uuid,
        note: Option<&str>,
        location: Option<GeoPoint>,
    ) -> Result<()> {
        let note = note.map(str::to_owned);
        self.device
            .lock()
            .await
            .complete_task(task, note, location)
            .await
    }

    async fn reopen(&self, task: Uuid) -> Result<()> {
        self.device.lock().await.reopen_task(task).await
    }

    async fn resolve(&self, task: Uuid, keep: Uuid) -> Result<()> {
        self.device.lock().await.resolve_conflict(task, keep).await
    }

    async fn task_id(&self, title: &str) -> Result<Uuid> {
        let state = self.state().await?;
        let task = state
            .task_by_title(title)
            .with_context(|| format!("no task {title:?}"))?;
        Ok(task.id)
    }

    /// Whether the task exists and its status satisfies `check`.
    async fn sees(&self, title: &str, check: impl Fn(&TaskStatus) -> bool) -> Result<bool> {
        let state = self.state().await?;
        Ok(state
            .task_by_title(title)
            .is_some_and(|task| check(&task.status)))
    }
}

/// The owner invites, the other phone joins after both compared the digits.
async fn pair(owner: &Phone, joiner: &mut Phone, joiner_name: &str) -> Result<()> {
    let ticket = owner.device.lock().await.create_invite().await?;
    let request = joiner
        .device
        .lock()
        .await
        .request_join(&ticket.link)
        .await?;
    let (device_id, confirmation) = loop {
        let progress = owner.device.lock().await.invite_progress(&ticket).await?;
        if let InviteProgress::Requested {
            device_id,
            confirmation,
        } = progress
        {
            break (device_id, confirmation);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    ensure!(
        confirmation == request.confirmation,
        "both phones must show the same digits"
    );
    ensure!(
        device_id == joiner.id().await,
        "the owner saw another phone"
    );
    owner
        .device
        .lock()
        .await
        .approve_join(&ticket, device_id)
        .await?;
    while !joiner
        .device
        .lock()
        .await
        .complete_join(&request, joiner_name)
        .await?
    {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    joiner.go_live();
    Ok(())
}

fn scratch_dir(name: &str) -> Result<PathBuf> {
    let dir = std::env::temp_dir().join(format!("tackly-e2e-{name}-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&dir).context("create a scratch directory")?;
    Ok(dir)
}

fn in_progress_by(who: Uuid) -> impl Fn(&TaskStatus) -> bool {
    move |status| matches!(status, TaskStatus::InProgress { by, .. } if *by == who)
}

fn is_done(status: &TaskStatus) -> bool {
    matches!(status, TaskStatus::Done { .. })
}

/// Patrick creates the family; Mona and Mara join.
async fn family_of_three(dir: &Path, relay: &Relay) -> Result<(Phone, Phone, Phone)> {
    let mut patrick = Phone::new("patrick", dir)?;
    patrick
        .device
        .lock()
        .await
        .create_family(&relay.url(), "The Smiths", "Patrick")
        .await?;
    patrick.go_live();
    let (mut mona, mut mara) = (Phone::new("mona", dir)?, Phone::new("mara", dir)?);
    pair(&patrick, &mut mona, "Mona").await?;
    pair(&patrick, &mut mara, "Mara").await?;
    Ok((patrick, mona, mara))
}

// ---- scenarios -------------------------------------------------------------

#[tokio::test]
async fn three_members_share_tasks_and_see_each_other_live() -> Result<()> {
    let dir = scratch_dir("live")?;
    let mut relay = Relay::reserve(&dir)?;
    relay.start().await?;
    let (patrick, mona, mara) = family_of_three(&dir, &relay).await?;
    let (patrick_id, mona_id, mara_id) = (patrick.id().await, mona.id().await, mara.id().await);

    // Everyone sees the family and all three members.
    for phone in [&patrick, &mona, &mara] {
        eventually("all members known", || async {
            let state = phone.state().await?;
            Ok(state.name.as_deref() == Some("The Smiths") && state.members.len() == 3)
        })
        .await?;
        let state = phone.state().await?;
        assert_eq!(state.member_name(patrick_id), "Patrick");
        assert!(state.members[&patrick_id].owner);
        assert_eq!(state.member_name(mona_id), "Mona");
        assert_eq!(state.member_name(mara_id), "Mara");
        assert!(!state.members[&mona_id].owner);
    }

    // The task list syncs.
    patrick.add_task("Do the dishes", "🍽").await?;
    patrick.add_task("Take out the trash", "🗑").await?;
    for phone in [&mona, &mara] {
        eventually("task list synced", || async {
            Ok(phone.state().await?.tasks.len() == 2)
        })
        .await?;
    }

    // Mona starts a task: the others see it live, without any refresh.
    let dishes = mona.task_id("Do the dishes").await?;
    mona.start(dishes).await?;
    for phone in [&patrick, &mara] {
        eventually("start visible", || async {
            phone.sees("Do the dishes", in_progress_by(mona_id)).await
        })
        .await?;
    }
    let activity = mara.state().await?.activity;
    assert!(
        activity
            .iter()
            .any(|a| a.by == mona_id && a.subject == "Do the dishes")
    );

    // Mona finishes it with metadata; everyone sees who, when, how long, where.
    tokio::time::sleep(Duration::from_millis(1100)).await;
    mona.complete(
        dishes,
        Some("Dishwasher was full"),
        Some(GeoPoint {
            latitude: 52.52,
            longitude: 13.405,
            accuracy_meters: Some(9.0),
        }),
    )
    .await?;
    for phone in [&patrick, &mara, &mona] {
        eventually("completion visible", || async {
            phone.sees("Do the dishes", is_done).await
        })
        .await?;
        let state = phone.state().await?;
        let TaskStatus::Done {
            by, metadata, at, ..
        } = &state.tasks[&dishes].status
        else {
            bail!("the task should be done");
        };
        assert_eq!(*by, mona_id);
        assert!(metadata.started_event_id.is_some());
        assert!(metadata.duration_seconds.context("a duration")? >= 1);
        assert_eq!(metadata.note.as_deref(), Some("Dishwasher was full"));
        assert_eq!(metadata.location.context("a location")?.latitude, 52.52);
        assert!(*at <= chrono::Utc::now());
    }

    // Mara does the same for the other task, Patrick watches.
    let trash = mara.task_id("Take out the trash").await?;
    mara.start(trash).await?;
    eventually("patrick sees mara start", || async {
        patrick
            .sees("Take out the trash", in_progress_by(mara_id))
            .await
    })
    .await?;
    mara.complete(trash, None, None).await?;
    eventually("patrick sees mara finish", || async {
        patrick.sees("Take out the trash", is_done).await
    })
    .await?;

    // Reopening propagates too.
    mona.reopen(dishes).await?;
    eventually("reopen visible", || async {
        mara.sees("Do the dishes", |s| matches!(s, TaskStatus::Open))
            .await
    })
    .await?;

    // All three replays are identical.
    let (a, b, c) = (
        patrick.state().await?,
        mona.state().await?,
        mara.state().await?,
    );
    assert_eq!(a.tasks, b.tasks);
    assert_eq!(b.tasks, c.tasks);
    assert_eq!(a.activity.len(), c.activity.len());
    Ok(())
}

#[tokio::test]
async fn double_completion_is_settled_by_a_member_who_took_part() -> Result<()> {
    let dir = scratch_dir("conflict")?;
    let mut relay = Relay::reserve(&dir)?;
    relay.start().await?;
    let (patrick, mona, mara) = family_of_three(&dir, &relay).await?;
    let mara_id = mara.id().await;

    patrick.add_task("Water the plants", "🪴").await?;
    for phone in [&mona, &mara] {
        eventually("task synced", || async {
            Ok(phone.state().await?.tasks.len() == 1)
        })
        .await?;
    }
    let task = patrick.task_id("Water the plants").await?;

    // Mona and Mara both finish it while the relay is down.
    relay.stop();
    mona.complete(task, Some("Mona did it"), None).await?;
    mara.complete(task, Some("Mara did it"), None).await?;
    relay.start().await?;

    for phone in [&patrick, &mona, &mara] {
        eventually("conflict visible", || async {
            Ok(phone.state().await?.tasks[&task].has_conflict())
        })
        .await?;
    }

    // Patrick did not take part, so he cannot settle it.
    let first_claim = patrick.state().await?.tasks[&task].claims[0].completion_event_id;
    assert!(patrick.resolve(task, first_claim).await.is_err());

    // Either participant can.
    let state = patrick.state().await?;
    let kept = state.tasks[&task]
        .claims
        .iter()
        .find(|claim| claim.by == mara_id)
        .context("Mara's claim")?
        .completion_event_id;
    mona.resolve(task, kept).await?;
    for phone in [&patrick, &mona, &mara] {
        eventually("conflict settled", || async {
            let state = phone.state().await?;
            let task = &state.tasks[&task];
            Ok(!task.has_conflict()
                && matches!(&task.status, TaskStatus::Done { by, .. } if *by == mara_id))
        })
        .await?;
    }
    Ok(())
}

#[tokio::test]
async fn a_double_completion_that_agrees_settles_itself() -> Result<()> {
    let dir = scratch_dir("auto-settle")?;
    let mut relay = Relay::reserve(&dir)?;
    relay.start().await?;
    let (patrick, mona, mara) = family_of_three(&dir, &relay).await?;

    patrick.add_task("Feed the cat", "🐱").await?;
    for phone in [&mona, &mara] {
        eventually("task synced", || async {
            Ok(phone.state().await?.tasks.len() == 1)
        })
        .await?;
    }
    let task = patrick.task_id("Feed the cat").await?;

    // Mona adds a note; Mara just taps finish. Nothing to decide.
    relay.stop();
    mona.complete(task, Some("Fed her"), None).await?;
    mara.complete(task, None, None).await?;
    relay.start().await?;

    for phone in [&patrick, &mona, &mara] {
        eventually("settled by itself", || async {
            let state = phone.state().await?;
            let task = &state.tasks[&task];
            Ok(task.is_done() && task.claims.len() == 1)
        })
        .await?;
        assert!(!phone.state().await?.tasks[&task].has_conflict());
    }
    Ok(())
}

#[tokio::test]
async fn everything_works_without_a_server_and_syncs_later() -> Result<()> {
    let dir = scratch_dir("offline")?;
    let mut relay = Relay::reserve(&dir)?; // not started: the relay is down

    let mut patrick = Phone::new("patrick", &dir)?;
    patrick
        .device
        .lock()
        .await
        .create_family(&relay.url(), "The Smiths", "Patrick")
        .await
        .context("a family can be created with no server")?;
    patrick.go_live();
    let patrick_id = patrick.id().await;

    let task = patrick.add_task("Pack the bags", "🧳").await?;
    patrick.start(task).await?;
    patrick.complete(task, Some("Done offline"), None).await?;
    let state = patrick.state().await?;
    assert_eq!(state.name.as_deref(), Some("The Smiths"));
    assert!(is_done(&state.tasks[&task].status));
    let registered = patrick
        .device
        .lock()
        .await
        .membership()
        .context("a membership")?
        .registered;
    assert!(!registered, "nothing was registered with the relay yet");
    // Pairing needs the relay and says so instead of pretending.
    assert!(patrick.device.lock().await.create_invite().await.is_err());

    // The relay comes up; Patrick's phone registers and uploads by itself.
    relay.start().await?;
    let mut mona = Phone::new("mona", &dir)?;
    eventually("invite once registered", || async {
        Ok(patrick.device.lock().await.create_invite().await.is_ok())
    })
    .await?;
    pair(&patrick, &mut mona, "Mona").await?;
    eventually("mona receives the offline history", || async {
        mona.sees("Pack the bags", is_done).await
    })
    .await?;
    let state = mona.state().await?;
    let TaskStatus::Done { by, metadata, .. } = &state.tasks[&task].status else {
        bail!("the task should be done");
    };
    assert_eq!(*by, patrick_id);
    assert_eq!(metadata.note.as_deref(), Some("Done offline"));
    assert!(metadata.duration_seconds.is_some());
    Ok(())
}

#[tokio::test]
async fn a_server_outage_does_not_stop_members_and_they_converge_afterwards() -> Result<()> {
    let dir = scratch_dir("outage")?;
    let mut relay = Relay::reserve(&dir)?;
    relay.start().await?;
    let (patrick, mona, mara) = family_of_three(&dir, &relay).await?;
    let mona_id = mona.id().await;

    patrick.add_task("Laundry", "🧺").await?;
    patrick.add_task("Groceries", "🛒").await?;
    for phone in [&mona, &mara] {
        eventually("tasks synced", || async {
            Ok(phone.state().await?.tasks.len() == 2)
        })
        .await?;
    }
    let laundry = patrick.task_id("Laundry").await?;
    let groceries = patrick.task_id("Groceries").await?;

    relay.stop();
    mona.start(laundry).await?;
    mara.start(groceries).await?;
    mara.complete(groceries, None, None).await?;
    patrick.add_task("Vacuum", "🧹").await?;
    // Phones without the relay see only their own changes, and keep working.
    assert!(mona.sees("Laundry", in_progress_by(mona_id)).await?);
    assert!(!patrick.sees("Laundry", in_progress_by(mona_id)).await?);
    assert!(mona.state().await?.task_by_title("Vacuum").is_none());

    relay.start().await?;
    for phone in [&patrick, &mona, &mara] {
        eventually("converged after the outage", || async {
            Ok(phone.sees("Laundry", in_progress_by(mona_id)).await?
                && phone.sees("Groceries", is_done).await?
                && phone.state().await?.task_by_title("Vacuum").is_some())
        })
        .await?;
    }
    let (a, b, c) = (
        patrick.state().await?,
        mona.state().await?,
        mara.state().await?,
    );
    assert_eq!(a.tasks, b.tasks);
    assert_eq!(b.tasks, c.tasks);
    Ok(())
}

/// A phone whose registration answer was lost asks again with the same token
/// and succeeds; a different token for the same family is refused.
#[tokio::test]
async fn registering_a_family_again_is_safe_only_with_the_same_token() -> Result<()> {
    let dir = scratch_dir("register")?;
    let mut relay = Relay::reserve(&dir)?;
    relay.start().await?;
    let api = Api::new(&relay.url())?;
    let (family, device) = (Uuid::new_v4(), Uuid::new_v4());

    api.create_family(family, device, "token-one").await?;
    api.create_family(family, device, "token-one").await?;
    assert!(
        api.create_family(family, device, "token-two")
            .await
            .is_err()
    );
    assert!(
        api.create_family(family, Uuid::new_v4(), "token-one")
            .await
            .is_err()
    );
    Ok(())
}
