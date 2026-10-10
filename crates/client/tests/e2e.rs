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
        .request_join(&ticket.code)
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
    let mut anna = Phone::new("anna", dir);
    anna.device
        .lock()
        .await
        .create_family(&server.url(), "The Smiths", "Anna")
        .await
        .unwrap();
    anna.go_live();
    let mut ben = Phone::new("ben", dir);
    let mut caro = Phone::new("caro", dir);
    pair(&anna, &mut ben, "Ben").await;
    pair(&anna, &mut caro, "Caro").await;
    (anna, ben, caro)
}

// ---- scenarios ---------------------------------------------------------

#[tokio::test]
async fn three_members_share_tasks_and_see_each_other_live() {
    let dir = scratch("live");
    let mut server = Server::reserve(&dir);
    server.start().await;
    let (anna, ben, caro) = family_of_three(&dir, &server).await;
    let (anna_id, ben_id, caro_id) = (anna.id().await, ben.id().await, caro.id().await);

    // Everyone sees the family and all three members.
    for phone in [&anna, &ben, &caro] {
        eventually("all members known", || async {
            let state = phone.state().await;
            state.name.as_deref() == Some("The Smiths") && state.members.len() == 3
        })
        .await;
        let state = phone.state().await;
        assert_eq!(state.member_name(anna_id), "Anna");
        assert!(state.members[&anna_id].owner);
        assert_eq!(state.member_name(ben_id), "Ben");
        assert_eq!(state.member_name(caro_id), "Caro");
        assert!(!state.members[&ben_id].owner);
    }

    // The task list syncs.
    anna.add_task("Do the dishes", "🍽").await;
    anna.add_task("Take out the trash", "🗑").await;
    for phone in [&ben, &caro] {
        eventually("task list synced", || async {
            phone.state().await.tasks.len() == 2
        })
        .await;
    }

    // Ben starts a task: the others see it live, without any manual refresh.
    let dishes = ben.task_id("Do the dishes").await;
    ben.start(dishes).await;
    for phone in [&anna, &caro] {
        eventually("start visible", || async {
            phone.sees("Do the dishes", in_progress_by(ben_id)).await
        })
        .await;
    }
    let activity = caro.state().await.activity;
    assert!(
        activity
            .iter()
            .any(|a| a.by == ben_id && a.subject == "Do the dishes")
    );

    // Ben finishes it with metadata; everyone sees who, when, how long, where.
    tokio::time::sleep(Duration::from_millis(1100)).await;
    ben.complete(
        dishes,
        Some("Dishwasher was full"),
        Some(GeoPoint {
            latitude: 52.52,
            longitude: 13.405,
            accuracy_meters: Some(9.0),
        }),
    )
    .await;
    for phone in [&anna, &caro, &ben] {
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
        assert_eq!(*by, ben_id);
        assert!(metadata.started_event_id.is_some());
        assert!(metadata.duration_seconds.unwrap() >= 1);
        assert_eq!(metadata.note.as_deref(), Some("Dishwasher was full"));
        assert_eq!(metadata.location.unwrap().latitude, 52.52);
        assert!(*at <= chrono::Utc::now());
    }

    // Caro does the same for the other task, Anna watches.
    let trash = caro.task_id("Take out the trash").await;
    caro.start(trash).await;
    eventually("anna sees caro start", || async {
        anna.sees("Take out the trash", in_progress_by(caro_id))
            .await
    })
    .await;
    caro.complete(trash, None, None).await;
    eventually("anna sees caro finish", || async {
        anna.sees("Take out the trash", is_done).await
    })
    .await;

    // Reopening propagates too.
    ben.reopen(dishes).await;
    eventually("reopen visible", || async {
        caro.sees("Do the dishes", |s| matches!(s, TaskStatus::Open))
            .await
    })
    .await;

    // All three replays are identical.
    let (a, b, c) = (anna.state().await, ben.state().await, caro.state().await);
    assert_eq!(a.tasks, b.tasks);
    assert_eq!(b.tasks, c.tasks);
    assert_eq!(a.activity.len(), c.activity.len());
}

#[tokio::test]
async fn double_completion_is_settled_by_a_member_who_took_part() {
    let dir = scratch("conflict");
    let mut server = Server::reserve(&dir);
    server.start().await;
    let (anna, ben, caro) = family_of_three(&dir, &server).await;
    let (ben_id, caro_id) = (ben.id().await, caro.id().await);

    anna.add_task("Water the plants", "🪴").await;
    for phone in [&ben, &caro] {
        eventually("task synced", || async {
            phone.state().await.tasks.len() == 1
        })
        .await;
    }
    let task = anna.task_id("Water the plants").await;

    // Ben and Caro both finish it while Caro is offline.
    server.stop();
    ben.complete(task, Some("Ben did it"), None).await;
    caro.complete(task, Some("Caro did it"), None).await;
    server.start().await;

    for phone in [&anna, &ben, &caro] {
        eventually("conflict visible", || async {
            phone.state().await.tasks[&task].has_conflict()
        })
        .await;
    }

    // Anna did not take part, so she cannot settle it.
    let first_claim = anna.state().await.tasks[&task].claims[0].completion_event_id;
    assert!(anna.resolve(task, first_claim).await.is_err());

    // Either participant can.
    let kept = anna.state().await.tasks[&task]
        .claims
        .iter()
        .find(|c| c.by == caro_id)
        .unwrap()
        .completion_event_id;
    ben.resolve(task, kept).await.unwrap();
    for phone in [&anna, &ben, &caro] {
        eventually("conflict settled", || async {
            let state = phone.state().await;
            let task = &state.tasks[&task];
            !task.has_conflict()
                && matches!(&task.status, TaskStatus::Done { by, .. } if *by == caro_id)
        })
        .await;
    }
    let _ = ben_id;
}

#[tokio::test]
async fn everything_works_without_a_server_and_syncs_later() {
    let dir = scratch("offline");
    let mut server = Server::reserve(&dir); // not started: the relay is down

    let mut anna = Phone::new("anna", &dir);
    anna.device
        .lock()
        .await
        .create_family(&server.url(), "The Smiths", "Anna")
        .await
        .expect("a family can be created with no server");
    anna.go_live();
    let anna_id = anna.id().await;

    anna.add_task("Pack the bags", "🧳").await;
    let task = anna.task_id("Pack the bags").await;
    anna.start(task).await;
    anna.complete(task, Some("Done offline"), None).await;
    let state = anna.state().await;
    assert_eq!(state.name.as_deref(), Some("The Smiths"));
    assert!(is_done(&state.tasks[&task].status));
    assert!(!anna.device.lock().await.membership().unwrap().registered());
    // Pairing needs the relay and says so instead of pretending.
    assert!(anna.device.lock().await.create_invite().await.is_err());

    // The relay comes up; Anna's phone registers and uploads by itself.
    server.start().await;
    let mut ben = Phone::new("ben", &dir);
    eventually("invite once registered", || async {
        let mut device = anna.device.lock().await;
        device.create_invite().await.is_ok()
    })
    .await;
    pair(&anna, &mut ben, "Ben").await;
    eventually("ben receives the offline history", || async {
        ben.sees("Pack the bags", is_done).await
    })
    .await;
    let state = ben.state().await;
    let TaskStatus::Done { by, metadata, .. } = &state.tasks[&task].status else {
        unreachable!()
    };
    assert_eq!(*by, anna_id);
    assert_eq!(metadata.note.as_deref(), Some("Done offline"));
    assert!(metadata.duration_seconds.is_some());
}

#[tokio::test]
async fn a_server_outage_does_not_stop_members_and_they_converge_afterwards() {
    let dir = scratch("outage");
    let mut server = Server::reserve(&dir);
    server.start().await;
    let (anna, ben, caro) = family_of_three(&dir, &server).await;
    let (anna_id, ben_id, caro_id) = (anna.id().await, ben.id().await, caro.id().await);

    anna.add_task("Laundry", "🧺").await;
    anna.add_task("Groceries", "🛒").await;
    for phone in [&ben, &caro] {
        eventually("tasks synced", || async {
            phone.state().await.tasks.len() == 2
        })
        .await;
    }
    let (laundry, groceries) = (
        anna.task_id("Laundry").await,
        anna.task_id("Groceries").await,
    );

    server.stop();
    ben.start(laundry).await;
    caro.start(groceries).await;
    caro.complete(groceries, None, None).await;
    anna.add_task("Vacuum", "🧹").await;
    // Offline phones see only their own changes, and keep working.
    assert!(ben.sees("Laundry", in_progress_by(ben_id)).await);
    assert!(!anna.sees("Laundry", in_progress_by(ben_id)).await);
    assert!(ben.state().await.task_by_title("Vacuum").is_none());

    server.start().await;
    for phone in [&anna, &ben, &caro] {
        eventually("converged after outage", || async {
            phone.sees("Laundry", in_progress_by(ben_id)).await
                && phone.sees("Groceries", is_done).await
                && phone.state().await.task_by_title("Vacuum").is_some()
        })
        .await;
    }
    let _ = (anna_id, caro_id);
    let (a, b, c) = (anna.state().await, ben.state().await, caro.state().await);
    assert_eq!(a.tasks, b.tasks);
    assert_eq!(b.tasks, c.tasks);
}
