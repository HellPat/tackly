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
use tackly_domain::{Family, GeoPoint, Photo, Picture, PlaceLocation, Progress, Task};
use tackly_phone::{
    Device, InviteProgress, SharedDevice,
    infrastructure::{device_token::DeviceToken, relay_client::Api},
    run_live,
};
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

fn winnenden() -> Result<GeoPoint> {
    Ok(GeoPoint::new(48.8752, 9.3775, Some(12.0))?)
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

    // Mona finishes; everyone sees it done. Where it happened follows once
    // her GPS knows, as on a real phone.
    mona.complete(dishes, None).await?;
    mona.device
        .lock()
        .await
        .locate_completion(dishes, winnenden()?)
        .await?;
    for phone in [&patrick, &mara] {
        eventually("dishes done, and where", || async {
            let state = phone.state().await?;
            Ok(state.tasks[&dishes]
                .done
                .as_ref()
                .is_some_and(|done| done.location.is_some()))
        })
        .await?;
        let state = phone.state().await?;
        let done = state.tasks[&dishes].done.as_ref().context("done")?;
        assert_eq!((done.by, done.location), (mona_id, Some(winnenden()?)));
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
                icon: "pets".parse()?,
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
        icon: "pets".parse()?,
        tint: 2,
        photo: Some(jpeg.parse()?),
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
    // A photo that is too big cannot even be made.
    ensure!(
        format!("data:image/jpeg;base64,{}", "A".repeat(200_000))
            .parse::<Photo>()
            .is_err(),
        "a huge photo is refused"
    );
    Ok(())
}

#[tokio::test]
async fn a_raised_problem_and_a_plan_sync_to_everyone() -> Result<()> {
    let dir = scratch_dir("problems")?;
    let mut relay = Relay::reserve(&dir)?;
    relay.start().await?;
    let (patrick, mona, mara) = family_of_three(&dir, &relay).await?;
    let mona_id = mona.id().await;

    // Patrick knows the problem and raises it for Mona.
    let problem = {
        let mut device = patrick.device.lock().await;
        let problem = device
            .create_problem("Out of food", "kitchen", &["Go shopping", "Cook pasta"])
            .await?;
        device.notice_problem(problem, mona_id).await?;
        problem
    };
    // Mona has it, with both steps.
    eventually("Mona has the problem and its steps", || async {
        let state = mona.state().await?;
        Ok(
            state.problems.get(&problem).and_then(|p| p.holder()) == Some(mona_id)
                && state
                    .steps(problem)
                    .filter(|t| t.assignee == Some(mona_id))
                    .count()
                    == 2,
        )
    })
    .await?;
    let shopping = mona.task_id("Go shopping").await?;

    // She plans tomorrow with one step and a free task, in her order.
    let free = patrick.add_task("Water the plants").await?;
    eventually("Mona sees the free task", || async {
        Ok(mona.state().await?.tasks.contains_key(&free))
    })
    .await?;
    let tomorrow = chrono::Utc::now().date_naive() + chrono::Duration::days(1);
    mona.device
        .lock()
        .await
        .plan_day(tomorrow, &[free, shopping])
        .await?;
    eventually(
        "the others see Mona's plan and that the free task is hers",
        || async {
            let state = mara.state().await?;
            Ok(state.plan(mona_id, tomorrow) == Some(&[free, shopping][..])
                && state.tasks[&free].assignee == Some(mona_id))
        },
    )
    .await?;

    // She resolves it; nobody sees it open any more.
    mona.device.lock().await.resolve_problem(problem).await?;
    eventually("Patrick sees it resolved", || async {
        let state = patrick.state().await?;
        Ok(state.open_problems().count() == 0 && state.steps(problem).count() == 0)
    })
    .await?;
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
            .create_place(
                group,
                "LIDL",
                PlaceLocation::named("LIDL Winnenden".parse()?),
            )
            .await?;
        device
            .add_place_location(lidl, PlaceLocation::named("LIDL Backnang".parse()?))
            .await?;
        let aldi = device
            .create_place(
                group,
                "Aldi",
                PlaceLocation::named("Aldi Waiblingen".parse()?),
            )
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
    patrick.complete(task, Some(winnenden()?)).await?;
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
    assert_eq!((done.by, done.location), (patrick_id, Some(winnenden()?)));
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
async fn a_relay_that_swaps_the_joiners_key_is_caught_by_the_digits() -> Result<()> {
    // Both phones derive their six digits from the invitation's secret and
    // the joiner's MLS key package. A hostile relay that puts its own key
    // package in Mona's place (here: Eve's) shows Patrick other digits than
    // Mona sees, so Patrick does not let it in.
    let dir = scratch_dir("hostile-relay")?;
    let mut relay = Relay::reserve(&dir)?;
    relay.start().await?;
    let mut patrick = Phone::new("patrick", &dir)?;
    patrick
        .device
        .lock()
        .await
        .create_family(&relay.url(), "The Smiths", "Patrick")
        .await?;
    patrick.go_live();
    let (mona, eve) = (Phone::new("mona", &dir)?, Phone::new("eve", &dir)?);

    let for_mona = patrick.device.lock().await.create_invite().await?;
    let for_eve = patrick.device.lock().await.create_invite().await?;
    let mona_asks = mona
        .device
        .lock()
        .await
        .request_join(&for_mona.link)
        .await?;
    eve.device.lock().await.request_join(&for_eve.link).await?;

    // The relay's doing: Eve's key package under Mona's invitation.
    let db = rusqlite::Connection::open(dir.join("relay.db"))?;
    db.execute(
        "UPDATE invitations SET key_package = (SELECT key_package FROM invitations WHERE id = ?1) WHERE id = ?2",
        [for_eve.invite_id.to_string(), for_mona.invite_id.to_string()],
    )?;

    let InviteProgress::Requested { confirmation, .. } = patrick
        .device
        .lock()
        .await
        .invite_progress(&for_mona)
        .await?
    else {
        bail!("Mona's request should be pending");
    };
    ensure!(
        confirmation != mona_asks.confirmation,
        "the digits must give the swap away"
    );
    Ok(())
}

#[tokio::test]
async fn the_head_removes_a_member_who_then_gets_nothing_new() -> Result<()> {
    let dir = scratch_dir("remove")?;
    let mut relay = Relay::reserve(&dir)?;
    relay.start().await?;
    let (patrick, mona, mara) = family_of_three(&dir, &relay).await?;
    patrick.add_task("Before").await?;
    eventually("Mara sees the task from before", || async {
        mara.sees("Before", |_| true).await
    })
    .await?;

    // Only the head removes anyone.
    let mara_id = mara.id().await;
    ensure!(
        mona.device
            .lock()
            .await
            .remove_member(mara_id)
            .await
            .is_err(),
        "Mona is not the head"
    );
    patrick.device.lock().await.remove_member(mara_id).await?;

    eventually("Patrick and Mona no longer list Mara", || async {
        Ok(!patrick.state().await?.members.contains_key(&mara_id)
            && !mona.state().await?.members.contains_key(&mara_id))
    })
    .await?;
    patrick.add_task("After").await?;
    eventually("Mona sees the task from after", || async {
        mona.sees("After", |_| true).await
    })
    .await?;
    // Mara keeps what she had, gets nothing new, and the relay turns her away.
    tokio::time::sleep(Duration::from_millis(500)).await;
    ensure!(mara.sees("Before", |_| true).await?, "the past stays");
    ensure!(!mara.sees("After", |_| true).await?, "nothing new for Mara");
    ensure!(
        mara.device.lock().await.pull().await.is_err(),
        "the relay refuses Mara's token"
    );
    ensure!(
        mara.device
            .lock()
            .await
            .membership()
            .is_some_and(|membership| membership.removed),
        "Mara's phone knows it was removed"
    );
    Ok(())
}

/// The family's ID, as the Family screen shows it to the head.
async fn family_id_of(phone: &Phone) -> Result<Uuid> {
    Ok(phone
        .device
        .lock()
        .await
        .membership()
        .context("not in a family")?
        .family_id)
}

#[tokio::test]
async fn an_ai_agent_asks_and_reads_the_whole_history_until_its_time_is_over() -> Result<()> {
    let dir = scratch_dir("agent")?;
    let mut relay = Relay::reserve(&dir)?;
    relay.start().await?;
    let (patrick, mona, _mara) = family_of_three(&dir, &relay).await?;
    let milk = patrick.add_task("Milk").await?;
    eventually("Mona sees Milk", || async {
        mona.sees("Milk", |_| true).await
    })
    .await?;
    mona.complete(milk, None).await?;
    eventually("Patrick sees Milk done", || async {
        patrick.sees("Milk", is_done).await
    })
    .await?;

    // The agent asks for access, knowing only the relay and the family's ID.
    let mut agent = Phone::new("agent", &dir)?;
    let family = family_id_of(&patrick).await?;
    let access = agent
        .device
        .lock()
        .await
        .ask_for_access(&relay.url(), family)
        .await?;
    ensure!(
        !agent.device.lock().await.complete_access(&access).await?,
        "not yet: the head has not decided"
    );
    let agent_id = agent.id().await;
    let ask = loop {
        let asks = patrick.device.lock().await.agent_asks().await?;
        if let Some(ask) = asks.into_iter().next() {
            break ask;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    ensure!(ask.code == access.code, "both sides show the same code");
    ensure!(
        mona.device.lock().await.agent_asks().await?.is_empty(),
        "only the head sees requests"
    );

    // The head allows it for two seconds.
    patrick
        .device
        .lock()
        .await
        .allow_agent(&ask, chrono::Duration::seconds(2))
        .await?;
    while !agent.device.lock().await.complete_access(&access).await? {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    // It has the whole history, is known as an agent, and cannot write.
    let seen = agent.state().await?;
    ensure!(seen.task_by_title("Milk").is_some_and(is_done), "history");
    ensure!(seen.people().count() == 3, "all three people");
    ensure!(seen.is_agent(agent_id), "known as an agent");
    ensure!(agent.add_task("Nope").await.is_err(), "an agent only reads");
    eventually("Mona lists the agent", || async {
        Ok(mona.state().await?.is_agent(agent_id))
    })
    .await?;
    agent.go_live();
    patrick.add_task("Eggs").await?;
    eventually("the agent sees new events live", || async {
        agent.sees("Eggs", |_| true).await
    })
    .await?;

    // Its time runs out: the head's phone takes it out; the relay is closed to it.
    tokio::time::sleep(Duration::from_millis(2_200)).await;
    patrick.device.lock().await.tick().await?;
    eventually("the agent is gone from the family", || async {
        Ok(!patrick.state().await?.members.contains_key(&agent_id)
            && !mona.state().await?.members.contains_key(&agent_id))
    })
    .await?;
    ensure!(
        agent.device.lock().await.pull().await.is_err(),
        "the relay turns the agent away"
    );
    Ok(())
}

#[tokio::test]
async fn a_denied_agent_is_turned_away() -> Result<()> {
    let dir = scratch_dir("agent-denied")?;
    let mut relay = Relay::reserve(&dir)?;
    relay.start().await?;
    let (patrick, _mona, _mara) = family_of_three(&dir, &relay).await?;
    let agent = Phone::new("agent", &dir)?;
    let family = family_id_of(&patrick).await?;
    let access = agent
        .device
        .lock()
        .await
        .ask_for_access(&relay.url(), family)
        .await?;
    let asks = patrick.device.lock().await.agent_asks().await?;
    let ask = asks.into_iter().next().context("the request is there")?;
    patrick.device.lock().await.deny_agent(&ask).await?;
    ensure!(
        agent
            .device
            .lock()
            .await
            .complete_access(&access)
            .await
            .is_err(),
        "denied"
    );
    ensure!(
        patrick.device.lock().await.agent_asks().await?.is_empty(),
        "nothing waits any more"
    );
    Ok(())
}

#[tokio::test]
async fn a_relay_that_swaps_the_agents_key_is_caught_by_the_code() -> Result<()> {
    let dir = scratch_dir("agent-swap")?;
    let mut relay = Relay::reserve(&dir)?;
    relay.start().await?;
    let (patrick, _mona, _mara) = family_of_three(&dir, &relay).await?;
    let family = family_id_of(&patrick).await?;
    let (agent, eve) = (Phone::new("agent", &dir)?, Phone::new("eve", &dir)?);
    let access = agent
        .device
        .lock()
        .await
        .ask_for_access(&relay.url(), family)
        .await?;
    let _ = eve
        .device
        .lock()
        .await
        .ask_for_access(&relay.url(), family)
        .await?;

    // The relay's doing: Eve's key package under the agent's request.
    let db = rusqlite::Connection::open(dir.join("relay.db"))?;
    let (agent_id, eve_id) = (agent.id().await.to_string(), eve.id().await.to_string());
    db.execute(
        "UPDATE agent_requests SET key_package = (SELECT key_package FROM agent_requests WHERE device_id = ?1) WHERE device_id = ?2",
        [eve_id, agent_id.clone()],
    )?;
    let asks = patrick.device.lock().await.agent_asks().await?;
    let for_agent = asks
        .iter()
        .find(|ask| ask.device_id.to_string() == agent_id)
        .context("the agent's request")?;
    ensure!(
        for_agent.code != access.code,
        "the code must give the swap away"
    );
    // And the phone refuses to add a key under another device's name.
    ensure!(
        patrick
            .device
            .lock()
            .await
            .allow_agent(for_agent, chrono::Duration::minutes(1))
            .await
            .is_err(),
        "a key that is not the agent's is refused"
    );
    Ok(())
}

#[tokio::test]
async fn only_a_few_agent_requests_wait_for_a_family() -> Result<()> {
    let dir = scratch_dir("agent-limit")?;
    let mut relay = Relay::reserve(&dir)?;
    relay.start().await?;
    let (patrick, _mona, _mara) = family_of_three(&dir, &relay).await?;
    let family = family_id_of(&patrick).await?;
    let mut refused = 0;
    for i in 0..7 {
        let agent = Phone::new(&format!("agent{i}"), &dir)?;
        let asked = agent
            .device
            .lock()
            .await
            .ask_for_access(&relay.url(), family)
            .await;
        refused += usize::from(asked.is_err());
    }
    ensure!(refused == 2, "five wait, the rest are refused");
    // A family that does not exist cannot be asked.
    let lost = Phone::new("lost", &dir)?;
    ensure!(
        lost.device
            .lock()
            .await
            .ask_for_access(&relay.url(), Uuid::new_v4())
            .await
            .is_err(),
        "no such family"
    );
    Ok(())
}

#[tokio::test]
async fn a_new_family_comes_with_a_recovery_code_the_head_must_confirm() -> Result<()> {
    let dir = scratch_dir("recovery-confirm")?;
    let mut relay = Relay::reserve(&dir)?;
    relay.start().await?;
    let (patrick, mona, _mara) = family_of_three(&dir, &relay).await?;

    // The code exists from the start and waits for the head to say it saved it.
    let first = patrick
        .device
        .lock()
        .await
        .pending_recovery_code()?
        .context("the family comes with a recovery code")?;
    ensure!(first.to_string().split(' ').count() == 24, "24 words");
    ensure!(
        mona.device.lock().await.pending_recovery_code()?.is_none(),
        "only the head has a code"
    );
    patrick.device.lock().await.confirm_recovery_code()?;
    ensure!(
        patrick
            .device
            .lock()
            .await
            .pending_recovery_code()?
            .is_none(),
        "saved"
    );

    // A new code must be confirmed again, and the old one stops working.
    let second = patrick.device.lock().await.new_recovery_code().await?;
    ensure!(second != first, "a different code");
    ensure!(
        patrick.device.lock().await.pending_recovery_code()? == Some(second.clone()),
        "the new code waits to be saved"
    );
    let stranger = Phone::new("stranger", &dir)?;
    ensure!(
        stranger
            .device
            .lock()
            .await
            .restore(&relay.url(), &first)
            .await
            .is_err(),
        "the old words open nothing"
    );
    Ok(())
}

#[tokio::test]
async fn the_head_comes_back_on_a_new_phone_with_the_recovery_code() -> Result<()> {
    let dir = scratch_dir("recovery")?;
    let mut relay = Relay::reserve(&dir)?;
    relay.start().await?;
    let (mut patrick, mona, mara) = family_of_three(&dir, &relay).await?;
    let code = patrick.device.lock().await.new_recovery_code().await?;
    ensure!(code.to_string().split(' ').count() == 24, "24 words");
    patrick.add_task("Milk").await?;
    eventually("Mona sees Milk", || async {
        mona.sees("Milk", |_| true).await
    })
    .await?;
    mona.add_task("Eggs").await?;
    eventually("Patrick sees Eggs", || async {
        patrick.sees("Eggs", |_| true).await
    })
    .await?;
    // Every flush on the head's phone refreshes the backup.
    patrick.device.lock().await.flush().await?;

    // Patrick's phone is lost (its sync stops); the family goes on meanwhile.
    let lost_id = patrick.id().await;
    if let Some(live) = patrick.live.take() {
        live.abort();
    }
    mona.add_task("Cheese").await?;
    eventually("Mara sees Cheese", || async {
        mara.sees("Cheese", |_| true).await
    })
    .await?;

    // A new phone with the words is the head again, with the whole history,
    // including what happened after the backup (the others send it).
    let mut back = Phone::new("patrick-again", &dir)?;
    // Other words are a code of their own, but open nothing.
    let other = tackly_phone::RecoveryCode::generate()?;
    ensure!(
        back.device
            .lock()
            .await
            .restore(&relay.url(), &other)
            .await
            .is_err(),
        "other words open nothing"
    );
    back.device
        .lock()
        .await
        .restore(&relay.url(), &code)
        .await?;
    back.go_live();
    ensure!(back.id().await == lost_id, "the same head");
    ensure!(
        back.device
            .lock()
            .await
            .membership()
            .is_some_and(|membership| membership.owner),
        "the head"
    );
    let state = back.state().await?;
    ensure!(
        state.task_by_title("Milk").is_some() && state.task_by_title("Eggs").is_some(),
        "the history from the backup"
    );
    ensure!(state.people().count() == 3, "all three people");
    eventually("the new phone has what came after the backup", || async {
        back.sees("Cheese", |_| true).await
    })
    .await?;

    // The family takes the new phone for the head, both ways.
    back.add_task("Bread").await?;
    eventually("Mona and Mara see Bread", || async {
        Ok(mona.sees("Bread", |_| true).await? && mara.sees("Bread", |_| true).await?)
    })
    .await?;
    mara.add_task("Butter").await?;
    eventually("the new phone sees Butter", || async {
        back.sees("Butter", |_| true).await
    })
    .await?;
    let mara_id = mara.id().await;
    back.device.lock().await.remove_member(mara_id).await?;
    eventually("Mona sees Mara gone", || async {
        Ok(!mona.state().await?.members.contains_key(&mara_id))
    })
    .await?;

    // The lost phone is out: the relay refuses its token, and whatever it
    // signs is no longer the head's.
    ensure!(
        patrick.device.lock().await.pull().await.is_err(),
        "the lost phone's token is dead"
    );
    ensure!(
        patrick.device.lock().await.create_invite().await.is_err(),
        "the lost phone cannot invite"
    );
    Ok(())
}

#[tokio::test]
async fn registering_a_family_again_is_safe_only_with_the_same_token() -> Result<()> {
    let dir = scratch_dir("register")?;
    let mut relay = Relay::reserve(&dir)?;
    relay.start().await?;
    let api = Api::new(&relay.url())?;
    let (family, device) = (Uuid::now_v7(), Uuid::now_v7());
    let (one, two) = (DeviceToken::generate(), DeviceToken::generate());

    api.create_family(family, device, &one).await?;
    api.create_family(family, device, &one).await?;
    assert!(api.create_family(family, device, &two).await.is_err());
    assert!(
        api.create_family(family, Uuid::now_v7(), &one)
            .await
            .is_err()
    );
    Ok(())
}
