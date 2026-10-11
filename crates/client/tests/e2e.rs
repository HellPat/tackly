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
use tackly_protocol::{Family, GeoPoint, Picture, PlaceLocation, Progress, Task};
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

    /// Adds a task to "Other".
    async fn add_task(&self, title: &str) -> Result<Uuid> {
        self.device.lock().await.add_task(title, None, &[]).await
    }

    async fn start(&self, task: Uuid) -> Result<()> {
        self.device.lock().await.start_task(task).await
    }

    async fn complete(&self, task: Uuid, location: Option<GeoPoint>) -> Result<()> {
        self.device.lock().await.complete_task(task, location).await
    }

    async fn task_id(&self, title: &str) -> Result<Uuid> {
        let state = self.state().await?;
        let task = state
            .task_by_title(title)
            .with_context(|| format!("no task {title:?}"))?;
        Ok(task.id)
    }

    /// Whether the task exists and satisfies `check`.
    async fn sees(&self, title: &str, check: impl Fn(&Task) -> bool) -> Result<bool> {
        let state = self.state().await?;
        Ok(state.task_by_title(title).is_some_and(check))
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
    let dir = std::env::temp_dir().join(format!("tackly-e2e-{name}-{}", Uuid::now_v7()));
    std::fs::create_dir_all(&dir).context("create a scratch directory")?;
    Ok(dir)
}

fn working(who: Uuid) -> impl Fn(&Task) -> bool {
    move |task| task.assignee == Some(who) && task.progress() == Some(Progress::Working)
}

fn is_done(task: &Task) -> bool {
    task.is_done()
}

const WINNENDEN: GeoPoint = GeoPoint {
    latitude: 48.8752,
    longitude: 9.3775,
    accuracy_meters: Some(12.0),
};

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
    let (mona_id, mara_id) = (mona.id().await, mara.id().await);

    // The task list syncs.
    let dishes = patrick.add_task("Do the dishes").await?;
    let trash = patrick.add_task("Take out the trash").await?;
    for phone in [&mona, &mara] {
        eventually("task list synced", || async {
            Ok(phone.state().await?.open_tasks_in(None).count() == 2)
        })
        .await?;
    }

    // Mona starts the dishes; the others see her working on it, live.
    mona.start(dishes).await?;
    for phone in [&patrick, &mara] {
        eventually("Mona seen working", || async {
            phone.sees("Do the dishes", working(mona_id)).await
        })
        .await?;
    }

    // Mona finishes; everyone sees it done, with where it happened.
    mona.complete(dishes, Some(WINNENDEN)).await?;
    for phone in [&patrick, &mara] {
        eventually("dishes done", || async {
            phone.sees("Do the dishes", is_done).await
        })
        .await?;
        let state = phone.state().await?;
        let done = state.tasks[&dishes].done.as_ref().context("done")?;
        assert_eq!((done.by, done.location), (mona_id, Some(WINNENDEN)));
        assert_eq!(state.tasks[&dishes].sessions.len(), 1, "her work was timed");
    }

    // Mara takes the trash without starting it.
    mara.device
        .lock()
        .await
        .assign_task(trash, Some(mara_id))
        .await?;
    eventually("Mara has the trash", || async {
        patrick
            .sees("Take out the trash", |task| {
                task.assignee == Some(mara_id) && task.progress() == Some(Progress::Picked)
            })
            .await
    })
    .await?;
    Ok(())
}

#[tokio::test]
async fn finished_twice_while_apart_is_simply_done() -> Result<()> {
    let dir = scratch_dir("twice")?;
    let mut relay = Relay::reserve(&dir)?;
    relay.start().await?;
    let (patrick, mona, mara) = family_of_three(&dir, &relay).await?;

    let cat = patrick.add_task("Feed the cat").await?;
    for phone in [&mona, &mara] {
        eventually("task synced", || async {
            phone.sees("Feed the cat", |_| true).await
        })
        .await?;
    }
    relay.stop();
    mona.complete(cat, None).await?;
    mara.complete(cat, None).await?;
    relay.start().await?;

    for phone in [&patrick, &mona, &mara] {
        eventually("done everywhere", || async {
            phone.sees("Feed the cat", is_done).await
        })
        .await?;
    }
    // Every phone agrees on who it counts for (whoever reached the relay first).
    let finisher = |family: Family| family.tasks[&cat].done.as_ref().map(|done| done.by);
    eventually("everyone agrees", || async {
        let (a, b, c) = (
            patrick.state().await?,
            mona.state().await?,
            mara.state().await?,
        );
        Ok(finisher(a.clone()) == finisher(b) && finisher(a) == finisher(c))
    })
    .await
}

#[tokio::test]
async fn giving_a_task_away_and_taking_it_back_syncs() -> Result<()> {
    let dir = scratch_dir("assign")?;
    let mut relay = Relay::reserve(&dir)?;
    relay.start().await?;
    let (patrick, mona, _mara) = family_of_three(&dir, &relay).await?;
    let mona_id = mona.id().await;

    let parcel = patrick.add_task("Pick up the parcel").await?;
    patrick
        .device
        .lock()
        .await
        .assign_task(parcel, Some(mona_id))
        .await?;
    eventually("Mona has it", || async {
        mona.sees("Pick up the parcel", |task| task.assignee == Some(mona_id))
            .await
    })
    .await?;
    // Mona can't hand it back for Patrick, but Patrick (who gave it) can: Undo.
    patrick
        .device
        .lock()
        .await
        .assign_task(parcel, None)
        .await?;
    eventually("free again", || async {
        mona.sees("Pick up the parcel", |task| task.assignee.is_none())
            .await
    })
    .await
}

#[tokio::test]
async fn lists_and_names_sync_to_everyone() -> Result<()> {
    let dir = scratch_dir("lists")?;
    let mut relay = Relay::reserve(&dir)?;
    relay.start().await?;
    let (patrick, mona, _mara) = family_of_three(&dir, &relay).await?;
    let mona_id = mona.id().await;

    let garden = patrick.device.lock().await.create_list("Garden").await?;
    let lawn = patrick
        .device
        .lock()
        .await
        .add_task("Mow the lawn", Some(garden), &[])
        .await?;
    patrick
        .device
        .lock()
        .await
        .rename_list(garden, "Backyard")
        .await?;
    {
        let mut device = mona.device.lock().await;
        device.rename_me("Mo").await?;
        device
            .set_picture(Picture {
                icon: "pets".into(),
                tint: 2,
                photo: None,
            })
            .await?;
    }
    eventually("everyone sees the list and the new name", || async {
        let (on_mona, on_patrick) = (mona.state().await?, patrick.state().await?);
        Ok(on_mona
            .lists
            .get(&garden)
            .is_some_and(|list| list.name == "Backyard")
            && on_mona.open_tasks_in(Some(garden)).count() == 1
            && on_patrick.member_name(mona_id) == "Mo"
            && on_patrick.members[&mona_id].picture.is_some())
    })
    .await?;

    // A list with open tasks stays; once they are done it can go.
    assert!(
        patrick
            .device
            .lock()
            .await
            .delete_list(garden)
            .await
            .is_err()
    );
    patrick.complete(lawn, None).await?;
    patrick.device.lock().await.delete_list(garden).await?;
    eventually("list gone everywhere", || async {
        Ok(!mona.state().await?.lists.contains_key(&garden))
    })
    .await
}

#[tokio::test]
async fn a_photo_as_picture_syncs_and_a_bad_one_is_refused() -> Result<()> {
    let dir = scratch_dir("photo")?;
    let mut relay = Relay::reserve(&dir)?;
    relay.start().await?;
    let (patrick, mona, _mara) = family_of_three(&dir, &relay).await?;
    let mona_id = mona.id().await;
    // A real (tiny) JPEG, as the phone makes it from the camera.
    let jpeg = "data:image/jpeg;base64,/9j/4AAQSkZJRgABAQAAAQABAAD/2wBDAAgGBgcGBQgHBwcJCQgKDBQNDAsLDBkSEw8UHRofHh0aHBwgJC4nICIsIxwcKDcpLDAxNDQ0Hyc5PTgyPC4zNDL/wAALCAABAAEBAREA/8QAFAABAAAAAAAAAAAAAAAAAAAACf/EABQQAQAAAAAAAAAAAAAAAAAAAAD/2gAIAQEAAD8AKp//2Q==";
    let picture = Picture {
        icon: "pets".into(),
        tint: 2,
        photo: Some(jpeg.into()),
    };
    mona.device
        .lock()
        .await
        .set_picture(picture.clone())
        .await?;
    eventually("Patrick sees Mona's photo", || async {
        Ok(patrick.state().await?.members[&mona_id].picture.as_ref() == Some(&picture))
    })
    .await?;
    let huge = Picture {
        photo: Some(format!("data:image/jpeg;base64,{}", "A".repeat(200_000))),
        ..picture
    };
    assert!(mona.device.lock().await.set_picture(huge).await.is_err());
    Ok(())
}

#[tokio::test]
async fn places_and_their_tasks_sync_to_everyone() -> Result<()> {
    let dir = scratch_dir("places")?;
    let mut relay = Relay::reserve(&dir)?;
    relay.start().await?;
    let (patrick, mona, mara) = family_of_three(&dir, &relay).await?;

    // Patrick sets up Grocery Store > LIDL (two branches) and Aldi.
    let (lidl, aldi, milk) = {
        let mut device = patrick.device.lock().await;
        let group = device.create_place_group("Grocery Store").await?;
        let lidl = device
            .create_place(group, "LIDL", PlaceLocation::named("LIDL Winnenden"))
            .await?;
        device
            .add_place_location(lidl, PlaceLocation::named("LIDL Backnang"))
            .await?;
        let aldi = device
            .create_place(group, "Aldi", PlaceLocation::named("Aldi Waiblingen"))
            .await?;
        let milk = device.add_task("Milk", None, &[lidl, aldi]).await?;
        device.add_task("Bread", None, &[lidl]).await?;
        (lidl, aldi, milk)
    };

    for phone in [&mona, &mara] {
        eventually("places and tasks synced", || async {
            let state = phone.state().await?;
            Ok(state.open_tasks_at(lidl).count() == 2
                && state.open_tasks_at(aldi).count() == 1
                && state
                    .places
                    .get(&lidl)
                    .is_some_and(|place| place.locations.len() == 2))
        })
        .await?;
    }

    // Mona buys the milk at Aldi: it is gone from LIDL's list as well.
    mona.complete(milk, None).await?;
    eventually("milk is off both lists", || async {
        let state = patrick.state().await?;
        Ok(state.open_tasks_at(lidl).count() == 1 && state.open_tasks_at(aldi).count() == 0)
    })
    .await?;

    // Mara moves the bread to Aldi.
    let bread = patrick.task_id("Bread").await?;
    {
        let mut device = mara.device.lock().await;
        device.add_task_to_place(bread, aldi).await?;
        device.remove_task_from_place(bread, lidl).await?;
    }
    eventually("bread moved", || async {
        let state = patrick.state().await?;
        Ok(state.open_tasks_at(lidl).count() == 0 && state.open_tasks_at(aldi).count() == 1)
    })
    .await
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

    let task = patrick.add_task("Pack the bags").await?;
    patrick.start(task).await?;
    patrick.complete(task, Some(WINNENDEN)).await?;
    let state = patrick.state().await?;
    assert_eq!(state.name.as_deref(), Some("The Smiths"));
    assert!(state.tasks[&task].is_done());
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
    let done = state.tasks[&task].done.as_ref().context("done")?;
    assert_eq!((done.by, done.location), (patrick_id, Some(WINNENDEN)));
    assert_eq!(state.tasks[&task].sessions.len(), 1);
    Ok(())
}

#[tokio::test]
async fn a_server_outage_does_not_stop_members_and_they_converge_afterwards() -> Result<()> {
    let dir = scratch_dir("outage")?;
    let mut relay = Relay::reserve(&dir)?;
    relay.start().await?;
    let (patrick, mona, mara) = family_of_three(&dir, &relay).await?;
    let mona_id = mona.id().await;

    let laundry = patrick.add_task("Laundry").await?;
    let groceries = patrick.add_task("Groceries").await?;
    for phone in [&mona, &mara] {
        eventually("tasks synced", || async {
            Ok(phone.state().await?.tasks.len() == 2)
        })
        .await?;
    }

    relay.stop();
    mona.start(laundry).await?;
    mara.start(groceries).await?;
    mara.complete(groceries, None).await?;
    patrick.add_task("Vacuum").await?;
    // Phones without the relay see only their own changes, and keep working.
    assert!(mona.sees("Laundry", working(mona_id)).await?);
    assert!(!patrick.sees("Laundry", working(mona_id)).await?);
    assert!(mona.state().await?.task_by_title("Vacuum").is_none());

    relay.start().await?;
    for phone in [&patrick, &mona, &mara] {
        eventually("converged after the outage", || async {
            Ok(phone.sees("Laundry", working(mona_id)).await?
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
    let (family, device) = (Uuid::now_v7(), Uuid::now_v7());

    api.create_family(family, device, "token-one").await?;
    api.create_family(family, device, "token-one").await?;
    assert!(
        api.create_family(family, device, "token-two")
            .await
            .is_err()
    );
    assert!(
        api.create_family(family, Uuid::now_v7(), "token-one")
            .await
            .is_err()
    );
    Ok(())
}
