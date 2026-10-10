//! End-to-end: real server (in-process, SQLite file), real HTTP and SSE, and
//! one real `Device` per phone, each with its own database and secrets.

use std::{
    future::Future,
    net::SocketAddr,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::Result;
use tackly_client::{Device, InviteProgress, SharedDevice, run_live};
use tackly_protocol::{Family, GeoPoint, TaskStatus};
use tokio::{sync::Mutex, task::JoinHandle};
use uuid::Uuid;

// ---- harness ---------------------------------------------------------

struct Server {
    addr: SocketAddr,
    db: String,
    runtime: Option<tokio::runtime::Runtime>,
}

impl Server {
    /// Reserves a port without serving on it, so the server can be "down".
    fn reserve(dir: &PathBuf) -> Self {
        let addr = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap();
        Self {
            addr,
            db: format!("sqlite://{}", dir.join("relay.db").display()),
            runtime: None,
        }
    }

    fn url(&self) -> String {
        format!("http://{}", self.addr)
    }

    async fn start(&mut self) {
        assert!(self.runtime.is_none(), "already running");
        // Own runtime, so `stop` also drops open SSE connections.
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let (db, addr) = (self.db.clone(), self.addr);
        let (ready, listening) = tokio::sync::oneshot::channel();
        runtime.spawn(async move {
            let pool = tackly_sync::connect(&db).await.unwrap();
            tackly_sync::migrate(&pool).await.unwrap();
            let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
            ready.send(()).unwrap();
            axum::serve(listener, tackly_sync::router(pool))
                .await
                .unwrap();
        });
        listening.await.expect("server failed to start");
        self.runtime = Some(runtime);
    }

    fn stop(&mut self) {
        self.runtime.take().expect("running").shutdown_background();
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        if let Some(runtime) = self.runtime.take() {
            runtime.shutdown_background();
        }
    }
}

async fn eventually<F: Future<Output = bool>>(what: &str, mut check: impl FnMut() -> F) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        if check().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("timed out waiting for: {what}");
}

struct Phone {
    name: &'static str,
    device: SharedDevice,
    live: Option<JoinHandle<()>>,
}

impl Phone {
    fn new(name: &'static str, dir: &PathBuf) -> Self {
        let dir = dir.join(name);
        Self {
            name,
            device: Arc::new(Mutex::new(Device::open(dir).unwrap())),
            live: None,
        }
    }

    /// Same as the app: sync keeps running while the phone is in a family.
    fn go_live(&mut self) {
        self.live = Some(tokio::spawn(run_live(self.device.clone(), || {})));
    }

    async fn state(&self) -> Family {
        self.device.lock().await.state().unwrap()
    }

    async fn id(&self) -> Uuid {
        self.device.lock().await.device_id()
    }

    // User actions succeed locally whatever the network does; the device's
    // upload trigger syncs them as soon as the relay is reachable.
    async fn add_task(&self, title: &str, emoji: &str) -> Uuid {
        self.device
            .lock()
            .await
            .add_task(title, emoji)
            .await
            .unwrap()
    }

    async fn start(&self, task: Uuid) {
        self.device.lock().await.start_task(task).await.unwrap();
    }

    async fn complete(&self, task: Uuid, note: Option<&str>, location: Option<GeoPoint>) {
        let note = note.map(str::to_owned);
        self.device
            .lock()
            .await
            .complete_task(task, note, location)
            .await
            .unwrap();
    }

    async fn reopen(&self, task: Uuid) {
        self.device.lock().await.reopen_task(task).await.unwrap();
    }

    async fn resolve(&self, task: Uuid, keep: Uuid) -> Result<()> {
        self.device.lock().await.resolve_conflict(task, keep).await
    }

    async fn task_id(&self, title: &str) -> Uuid {
        self.state().await.task_by_title(title).unwrap().id
    }

    async fn sees(&self, title: &str, check: impl Fn(&TaskStatus) -> bool) -> bool {
        self.state()
            .await
            .task_by_title(title)
            .is_some_and(|task| check(&task.status))
    }
}

/// Owner invites, the other phone joins after comparing the 6-digit codes.
async fn pair(owner: &Phone, joiner: &mut Phone, joiner_name: &str) {
    let ticket = owner.device.lock().await.create_invite().await.unwrap();
    let request = joiner
        .device
        .lock()
        .await
        .request_join(&ticket.link)
        .await
        .unwrap();
    let (device_id, confirmation) = loop {
        let progress = owner
            .device
            .lock()
            .await
            .invite_progress(&ticket)
            .await
            .unwrap();
        if let InviteProgress::Requested {
            device_id,
            confirmation,
        } = progress
        {
            break (device_id, confirmation);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    assert_eq!(
        confirmation, request.confirmation,
        "both phones show the same code"
    );
    assert_eq!(device_id, joiner.id().await);
    owner
        .device
        .lock()
        .await
        .approve_join(&ticket, device_id)
        .await
        .unwrap();
    while !joiner
        .device
        .lock()
        .await
        .complete_join(&request, joiner_name)
        .await
        .unwrap()
    {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    joiner.go_live();
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tackly-e2e-{name}-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn in_progress_by(who: Uuid) -> impl Fn(&TaskStatus) -> bool {
    move |status| matches!(status, TaskStatus::InProgress { by, .. } if *by == who)
}

fn is_done(status: &TaskStatus) -> bool {
    matches!(status, TaskStatus::Done { .. })
}

async fn family_of_three(dir: &PathBuf, server: &Server) -> (Phone, Phone, Phone) {
    let mut patrick = Phone::new("patrick", dir);
    patrick
        .device
        .lock()
        .await
        .create_family(&server.url(), "The Smiths", "Patrick")
        .await
        .unwrap();
    patrick.go_live();
    let mut mona = Phone::new("mona", dir);
    let mut mara = Phone::new("mara", dir);
    pair(&patrick, &mut mona, "Mona").await;
    pair(&patrick, &mut mara, "Mara").await;
    (patrick, mona, mara)
}

// ---- scenarios ---------------------------------------------------------

#[tokio::test]
async fn three_members_share_tasks_and_see_each_other_live() {
    let dir = scratch("live");
    let mut server = Server::reserve(&dir);
    server.start().await;
    let (patrick, mona, mara) = family_of_three(&dir, &server).await;
    let (patrick_id, mona_id, mara_id) = (patrick.id().await, mona.id().await, mara.id().await);

    // Everyone sees the family and all three members.
    for phone in [&patrick, &mona, &mara] {
        eventually("all members known", || async {
            let state = phone.state().await;
            state.name.as_deref() == Some("The Smiths") && state.members.len() == 3
        })
        .await;
        let state = phone.state().await;
        assert_eq!(state.member_name(patrick_id), "Patrick");
        assert!(state.members[&patrick_id].owner);
        assert_eq!(state.member_name(mona_id), "Mona");
        assert_eq!(state.member_name(mara_id), "Mara");
        assert!(!state.members[&mona_id].owner);
    }

    // The task list syncs.
    patrick.add_task("Do the dishes", "🍽").await;
    patrick.add_task("Take out the trash", "🗑").await;
    for phone in [&mona, &mara] {
        eventually("task list synced", || async {
            phone.state().await.tasks.len() == 2
        })
        .await;
    }

    // Mona starts a task: the others see it live, without any manual refresh.
    let dishes = mona.task_id("Do the dishes").await;
    mona.start(dishes).await;
    for phone in [&patrick, &mara] {
        eventually("start visible", || async {
            phone.sees("Do the dishes", in_progress_by(mona_id)).await
        })
        .await;
    }
    let activity = mara.state().await.activity;
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
    .await;
    for phone in [&patrick, &mara, &mona] {
        eventually("completion visible", || async {
            phone.sees("Do the dishes", is_done).await
        })
        .await;
        let state = phone.state().await;
        let TaskStatus::Done {
            by, metadata, at, ..
        } = &state.tasks[&dishes].status
        else {
            unreachable!()
        };
        assert_eq!(*by, mona_id);
        assert!(metadata.started_event_id.is_some());
        assert!(metadata.duration_seconds.unwrap() >= 1);
        assert_eq!(metadata.note.as_deref(), Some("Dishwasher was full"));
        assert_eq!(metadata.location.unwrap().latitude, 52.52);
        assert!(*at <= chrono::Utc::now());
    }

    // Mara does the same for the other task, Patrick watches.
    let trash = mara.task_id("Take out the trash").await;
    mara.start(trash).await;
    eventually("patrick sees mara start", || async {
        patrick
            .sees("Take out the trash", in_progress_by(mara_id))
            .await
    })
    .await;
    mara.complete(trash, None, None).await;
    eventually("patrick sees mara finish", || async {
        patrick.sees("Take out the trash", is_done).await
    })
    .await;

    // Reopening propagates too.
    mona.reopen(dishes).await;
    eventually("reopen visible", || async {
        mara.sees("Do the dishes", |s| matches!(s, TaskStatus::Open))
            .await
    })
    .await;

    // All three replays are identical.
    let (a, b, c) = (
        patrick.state().await,
        mona.state().await,
        mara.state().await,
    );
    assert_eq!(a.tasks, b.tasks);
    assert_eq!(b.tasks, c.tasks);
    assert_eq!(a.activity.len(), c.activity.len());
}

#[tokio::test]
async fn double_completion_is_settled_by_a_member_who_took_part() {
    let dir = scratch("conflict");
    let mut server = Server::reserve(&dir);
    server.start().await;
    let (patrick, mona, mara) = family_of_three(&dir, &server).await;
    let (mona_id, mara_id) = (mona.id().await, mara.id().await);

    patrick.add_task("Water the plants", "🪴").await;
    for phone in [&mona, &mara] {
        eventually("task synced", || async {
            phone.state().await.tasks.len() == 1
        })
        .await;
    }
    let task = patrick.task_id("Water the plants").await;

    // Mona and Mara both finish it while Mara is offline.
    server.stop();
    mona.complete(task, Some("Mona did it"), None).await;
    mara.complete(task, Some("Mara did it"), None).await;
    server.start().await;

    for phone in [&patrick, &mona, &mara] {
        eventually("conflict visible", || async {
            phone.state().await.tasks[&task].has_conflict()
        })
        .await;
    }

    // Patrick did not take part, so she cannot settle it.
    let first_claim = patrick.state().await.tasks[&task].claims[0].completion_event_id;
    assert!(patrick.resolve(task, first_claim).await.is_err());

    // Either participant can.
    let kept = patrick.state().await.tasks[&task]
        .claims
        .iter()
        .find(|c| c.by == mara_id)
        .unwrap()
        .completion_event_id;
    mona.resolve(task, kept).await.unwrap();
    for phone in [&patrick, &mona, &mara] {
        eventually("conflict settled", || async {
            let state = phone.state().await;
            let task = &state.tasks[&task];
            !task.has_conflict()
                && matches!(&task.status, TaskStatus::Done { by, .. } if *by == mara_id)
        })
        .await;
    }
    let _ = mona_id;
}

#[tokio::test]
async fn everything_works_without_a_server_and_syncs_later() {
    let dir = scratch("offline");
    let mut server = Server::reserve(&dir); // not started: the relay is down

    let mut patrick = Phone::new("patrick", &dir);
    patrick
        .device
        .lock()
        .await
        .create_family(&server.url(), "The Smiths", "Patrick")
        .await
        .expect("a family can be created with no server");
    patrick.go_live();
    let patrick_id = patrick.id().await;

    patrick.add_task("Pack the bags", "🧳").await;
    let task = patrick.task_id("Pack the bags").await;
    patrick.start(task).await;
    patrick.complete(task, Some("Done offline"), None).await;
    let state = patrick.state().await;
    assert_eq!(state.name.as_deref(), Some("The Smiths"));
    assert!(is_done(&state.tasks[&task].status));
    assert!(
        !patrick
            .device
            .lock()
            .await
            .membership()
            .unwrap()
            .registered()
    );
    // Pairing needs the relay and says so instead of pretending.
    assert!(patrick.device.lock().await.create_invite().await.is_err());

    // The relay comes up; Patrick's phone registers and uploads by itself.
    server.start().await;
    let mut mona = Phone::new("mona", &dir);
    eventually("invite once registered", || async {
        let mut device = patrick.device.lock().await;
        device.create_invite().await.is_ok()
    })
    .await;
    pair(&patrick, &mut mona, "Mona").await;
    eventually("mona receives the offline history", || async {
        mona.sees("Pack the bags", is_done).await
    })
    .await;
    let state = mona.state().await;
    let TaskStatus::Done { by, metadata, .. } = &state.tasks[&task].status else {
        unreachable!()
    };
    assert_eq!(*by, patrick_id);
    assert_eq!(metadata.note.as_deref(), Some("Done offline"));
    assert!(metadata.duration_seconds.is_some());
}

#[tokio::test]
async fn a_server_outage_does_not_stop_members_and_they_converge_afterwards() {
    let dir = scratch("outage");
    let mut server = Server::reserve(&dir);
    server.start().await;
    let (patrick, mona, mara) = family_of_three(&dir, &server).await;
    let (patrick_id, mona_id, mara_id) = (patrick.id().await, mona.id().await, mara.id().await);

    patrick.add_task("Laundry", "🧺").await;
    patrick.add_task("Groceries", "🛒").await;
    for phone in [&mona, &mara] {
        eventually("tasks synced", || async {
            phone.state().await.tasks.len() == 2
        })
        .await;
    }
    let (laundry, groceries) = (
        patrick.task_id("Laundry").await,
        patrick.task_id("Groceries").await,
    );

    server.stop();
    mona.start(laundry).await;
    mara.start(groceries).await;
    mara.complete(groceries, None, None).await;
    patrick.add_task("Vacuum", "🧹").await;
    // Offline phones see only their own changes, and keep working.
    assert!(mona.sees("Laundry", in_progress_by(mona_id)).await);
    assert!(!patrick.sees("Laundry", in_progress_by(mona_id)).await);
    assert!(mona.state().await.task_by_title("Vacuum").is_none());

    server.start().await;
    for phone in [&patrick, &mona, &mara] {
        eventually("converged after outage", || async {
            phone.sees("Laundry", in_progress_by(mona_id)).await
                && phone.sees("Groceries", is_done).await
                && phone.state().await.task_by_title("Vacuum").is_some()
        })
        .await;
    }
    let _ = (patrick_id, mara_id);
    let (a, b, c) = (
        patrick.state().await,
        mona.state().await,
        mara.state().await,
    );
    assert_eq!(a.tasks, b.tasks);
    assert_eq!(b.tasks, c.tasks);
}
