//! Cucumber acceptance tests. Every step is done like a person would, with
//! Playwright-style locators in real Tackly app windows (one process per family
//! member) against a real sync server; nothing talks to the device core.

mod support;

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::Command,
    sync::OnceLock,
    time::Duration,
};

use cucumber::{World, given, then, when};
use support::{
    member::Member,
    page::{Locator, Page, expect},
    server::Server,
};

static APP: OnceLock<PathBuf> = OnceLock::new();

/// Delay between key presses when typing key by key.
const KEY_DELAY: Duration = Duration::from_millis(12);

#[derive(World)]
#[world(init = Self::new)]
struct Tackly {
    dir: PathBuf,
    server: Server,
    members: HashMap<String, Member>,
}

impl std::fmt::Debug for Tackly {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tackly")
            .field("members", &self.members.keys())
            .finish()
    }
}

fn names(list: &str) -> Vec<String> {
    list.replace(" and ", ", ")
        .split(", ")
        .map(str::to_owned)
        .collect()
}

fn location(name: &str) -> &'static str {
    match name {
        "Patrick" => "52.5200,13.4050",
        "Mona" => "52.5163,13.3777",
        _ => "52.5075,13.3904",
    }
}

fn card(page: &Page, title: &str) -> Locator {
    page.locator(".card").filter_has_text(title)
}

#[derive(Clone, Copy)]
enum Via {
    Link,
    Qr,
}

impl Tackly {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("tackly-acceptance-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        Self {
            server: Server::reserve(&dir),
            dir,
            members: HashMap::new(),
        }
    }

    fn page(&self, name: &str) -> Page {
        self.members
            .get(name)
            .unwrap_or_else(|| panic!("{name} has not opened Tackly"))
            .page
            .clone()
    }

    async fn open(&mut self, name: &str) {
        let member = Member::open(
            name,
            APP.get().unwrap(),
            &self.dir,
            &self.server.url(),
            location(name),
        )
        .await;
        self.members.insert(name.to_owned(), member);
    }

    async fn go(&self, name: &str, tab: &str) {
        self.page(name)
            .get_by_role_exact("button", tab)
            .click()
            .await;
    }

    async fn create_family(&self, name: &str, family: &str) {
        let p = self.page(name);
        p.get_by_role_exact("button", "Create a family")
            .click()
            .await;
        p.get_by_label("Your name")
            .press_sequentially(name, KEY_DELAY)
            .await;
        p.get_by_label("Family name")
            .press_sequentially(family, KEY_DELAY)
            .await;
        p.get_by_role_exact("button", "Create").click().await;
        expect(&p.locator(".sub")).to_contain_text(family).await;
    }

    /// The owner shows an invitation; the other phone gets it by scanning the QR
    /// code or by pasting the copied link, both phones compare the six digits,
    /// and the owner lets them in.
    async fn invite(&self, owner: &str, joiner: &str, via: Via) {
        let (o, j) = (self.page(owner), self.page(joiner));
        j.get_by_role_exact("button", "Join with an invitation")
            .click()
            .await;
        o.get_by_role_exact("button", "Family").click().await;
        o.get_by_role_exact("button", "Invite someone")
            .click()
            .await;
        let shown = o.get_by_label("Invitation link").input_value().await;
        let link = match via {
            Via::Qr => {
                let scanned = o.locator(".qr").decode_qr().await;
                assert_eq!(
                    scanned, shown,
                    "the QR code must carry the same link that is shown"
                );
                scanned
            }
            Via::Link => {
                o.get_by_role_exact("button", "Copy link").click().await;
                expect(&o.locator(".snack"))
                    .to_contain_text("Link copied")
                    .await;
                o.get_by_role_exact("button", "OK").click().await;
                shown
            }
        };
        j.get_by_label("Your name")
            .press_sequentially(joiner, KEY_DELAY)
            .await;
        j.get_by_label("Invitation link").fill(&link).await;
        j.get_by_role_exact("button", "Ask to join").click().await;
        let digits = j.locator(".code").text_content().await;
        expect(&o.locator(".code")).to_have_text(&digits).await;
        o.get_by_role_exact("button", "Yes, let them in")
            .click()
            .await;
        expect(&j.get_by_role_exact("button", "Tasks"))
            .to_be_visible()
            .await;
        expect(&o.get_by_role_exact("button", "Yes, let them in"))
            .to_be_hidden()
            .await;
    }

    async fn add_task(&self, name: &str, title: &str) {
        self.go(name, "Tasks").await;
        let p = self.page(name);
        p.get_by_role_exact("button", "New task").click().await;
        p.get_by_label("What needs doing?")
            .press_sequentially(title, KEY_DELAY)
            .await;
        p.get_by_role_exact("button", "Add").click().await;
        expect(&card(&p, title)).to_be_visible().await;
    }

    async fn start(&self, name: &str, title: &str) {
        self.go(name, "Tasks").await;
        card(&self.page(name), title)
            .get_by_role_exact("button", "Start")
            .click()
            .await;
    }

    async fn finish(&self, name: &str, title: &str, note: Option<&str>) {
        self.go(name, "Tasks").await;
        let p = self.page(name);
        card(&p, title)
            .get_by_role_exact("button", "Finish")
            .click()
            .await;
        if let Some(note) = note {
            p.get_by_label("Note (optional)")
                .press_sequentially(note, KEY_DELAY)
                .await;
        }
        p.get_by_role_exact("button", "Done").click().await;
    }

    async fn see_card(&self, who: &[String], title: &str, parts: impl Fn(&str) -> Vec<String>) {
        for name in who {
            self.go(name, "Tasks").await;
            let p = self.page(name);
            for part in parts(name) {
                expect(&card(&p, title)).to_contain_text(&part).await;
            }
            expect(&card(&p, title)).to_be_visible().await;
        }
    }
}

// ---- Given / When ------------------------------------------------------

#[given("the sync server is running")]
#[when("the sync server is started")]
async fn server_running(world: &mut Tackly) {
    world.server.start().await;
}

#[given("the sync server is stopped")]
#[when("the sync server is stopped")]
async fn server_stopped(world: &mut Tackly) {
    world.server.stop();
}

#[given(regex = r"^(\w+), (\w+) and (\w+) have opened Tackly$")]
async fn opened(world: &mut Tackly, a: String, b: String, c: String) {
    for name in [a, b, c] {
        world.open(&name).await;
    }
}

#[given(regex = r#"^(\w+) has created the family "([^"]+)" with (\w+) and (\w+)$"#)]
async fn family_of_three(world: &mut Tackly, owner: String, family: String, a: String, b: String) {
    world.create_family(&owner, &family).await;
    world.invite(&owner, &a, Via::Link).await;
    world.invite(&owner, &b, Via::Link).await;
}

#[given(regex = r#"^(\w+) has created the family "([^"]+)"$"#)]
#[when(regex = r#"^(\w+) creates the family "([^"]+)"$"#)]
async fn creates_family(world: &mut Tackly, name: String, family: String) {
    world.create_family(&name, &family).await;
}

#[when(regex = r"^(\w+) invites (\w+)(?: by (QR code|link))?$")]
async fn invites(world: &mut Tackly, owner: String, joiner: String, via: String) {
    let via = if via == "QR code" { Via::Qr } else { Via::Link };
    world.invite(&owner, &joiner, via).await;
}

#[given(regex = r#"^(\w+) has added the tasks? ((?:"[^"]+"(?:, | and )?)+)$"#)]
#[when(regex = r#"^(\w+) adds the tasks? ((?:"[^"]+"(?:, | and )?)+)$"#)]
async fn adds_tasks(world: &mut Tackly, name: String, titles: String) {
    for title in titles.split('"').skip(1).step_by(2) {
        world.add_task(&name, title).await;
    }
}

#[when(regex = r#"^(\w+) starts "([^"]+)"$"#)]
async fn starts(world: &mut Tackly, name: String, title: String) {
    world.start(&name, &title).await;
}

#[when(regex = r#"^(\w+) finishes "([^"]+)"$"#)]
async fn finishes(world: &mut Tackly, name: String, title: String) {
    world.finish(&name, &title, None).await;
}

#[when(regex = r#"^(\w+) finishes "([^"]+)" with the note "([^"]+)"$"#)]
async fn finishes_with_note(world: &mut Tackly, name: String, title: String, note: String) {
    world.finish(&name, &title, Some(&note)).await;
}

#[when(regex = r#"^(\w+) reopens "([^"]+)"$"#)]
async fn reopens(world: &mut Tackly, name: String, title: String) {
    world.go(&name, "Tasks").await;
    card(&world.page(&name), &title)
        .get_by_role_exact("button", "Reopen")
        .click()
        .await;
}

#[when(regex = r#"^(\w+) keeps (\w+)'s completion of "([^"]+)"$"#)]
async fn keeps(world: &mut Tackly, name: String, winner: String, title: String) {
    world.go(&name, "Tasks").await;
    card(&world.page(&name), &title)
        .get_by_role_exact("button", &format!("Keep {winner}'s"))
        .click()
        .await;
}

// ---- keyboard ------------------------------------------------------------------

#[when(regex = r"^(\w+) opens the new task form$")]
async fn opens_new_task(world: &mut Tackly, name: String) {
    world.go(&name, "Tasks").await;
    world
        .page(&name)
        .get_by_role_exact("button", "New task")
        .click()
        .await;
}

#[when(regex = r#"^(\w+) types "([^"]*)" into "([^"]+)" key by key$"#)]
async fn types_key_by_key(world: &mut Tackly, name: String, text: String, label: String) {
    world
        .page(&name)
        .get_by_label(&label)
        .press_sequentially(&text, KEY_DELAY)
        .await;
}

#[when(regex = r#"^(\w+) presses (\w+)(?: (\d+) times)? in "([^"]+)"$"#)]
async fn presses_key(world: &mut Tackly, name: String, key: String, times: String, label: String) {
    let field = world.page(&name).get_by_label(&label);
    for _ in 0..times.parse::<usize>().unwrap_or(1) {
        field.press(&key).await;
    }
}

#[then(regex = r#"^the field "([^"]+)" of (\w+) contains "([^"]*)"$"#)]
async fn field_contains(world: &mut Tackly, label: String, name: String, value: String) {
    expect(&world.page(&name).get_by_label(&label))
        .to_have_value(&value)
        .await;
}

#[then(regex = r#"^the "([^"]+)" button of (\w+) is (enabled|disabled)$"#)]
async fn button_state(world: &mut Tackly, label: String, name: String, state: String) {
    let button = world.page(&name).get_by_role_exact("button", &label);
    if state == "enabled" {
        expect(&button).to_be_enabled().await;
    } else {
        expect(&button).to_be_disabled().await;
    }
}

// ---- Then --------------------------------------------------------------

#[given(regex = r#"^(.+?) sees? the tasks? ((?:"[^"]+"(?:, | and )?)+)$"#)]
#[then(regex = r#"^(.+?) sees? the tasks? ((?:"[^"]+"(?:, | and )?)+)$"#)]
async fn sees_tasks(world: &mut Tackly, who: String, titles: String) {
    for title in titles.split('"').skip(1).step_by(2) {
        world.see_card(&names(&who), title, |_| vec![]).await;
    }
}

#[then(regex = r#"^(.+?) sees? "([^"]+)" in progress by (\w+)$"#)]
async fn sees_in_progress(world: &mut Tackly, who: String, title: String, by: String) {
    world
        .see_card(&names(&who), &title, |viewer| {
            vec![if viewer == by {
                "You are on it".into()
            } else {
                format!("{by} is on it")
            }]
        })
        .await;
}

#[then(regex = r#"^(.+?) sees? "([^"]+)" done by (\w+)$"#)]
async fn sees_done(world: &mut Tackly, who: String, title: String, by: String) {
    world
        .see_card(&names(&who), &title, |viewer| {
            vec![format!(
                "Done by {}",
                if viewer == by { "You" } else { &by }
            )]
        })
        .await;
}

#[then(
    regex = r#"^(.+?) sees? "([^"]+)" done by (\w+) with a duration, the note "([^"]+)" and a location$"#
)]
async fn sees_done_with_metadata(
    world: &mut Tackly,
    who: String,
    title: String,
    by: String,
    note: String,
) {
    world
        .see_card(&names(&who), &title, |viewer| {
            vec![
                format!("Done by {}", if viewer == by { "You" } else { &by }),
                "⏱".into(),
                format!("💬 {note}"),
                "📍 52.".into(),
            ]
        })
        .await;
}

#[then(regex = r#"^(.+?) sees? "([^"]+)" open again$"#)]
async fn sees_open(world: &mut Tackly, who: String, title: String) {
    for name in names(&who) {
        world.go(&name, "Tasks").await;
        let c = card(&world.page(&name), &title);
        expect(&c.get_by_role_exact("button", "Start"))
            .to_be_visible()
            .await;
        expect(&c).not().to_contain_text("Done by").await;
    }
}

#[then(regex = r#"^(.+?) sees? "([^"]+)" finished twice$"#)]
async fn sees_conflict(world: &mut Tackly, who: String, title: String) {
    world
        .see_card(&names(&who), &title, |_| vec!["finished twice".into()])
        .await;
}

#[then(regex = r#"^(\w+) can only wait for (\w+) and (\w+) to decide "([^"]+)"$"#)]
async fn cannot_decide(world: &mut Tackly, name: String, a: String, b: String, title: String) {
    world.go(&name, "Tasks").await;
    let c = card(&world.page(&name), &title);
    for part in ["Waiting for", &a, &b, "to decide"] {
        expect(&c).to_contain_text(part).await;
    }
    expect(&c.get_by_role("button", "Keep"))
        .to_have_count(0)
        .await;
}

#[then(regex = r"^(.+?) sees? the members (.+) with (\w+) as head of the family$")]
async fn sees_members(world: &mut Tackly, who: String, members: String, head: String) {
    for name in names(&who) {
        world.go(&name, "Family").await;
        let p = world.page(&name);
        for member in names(&members) {
            expect(&card(&p, &member)).to_be_visible().await;
        }
        expect(&card(&p, &head))
            .to_contain_text("Head of the family")
            .await;
    }
}

#[then(regex = r"^(\w+)'s app says it is offline$")]
async fn says_offline(world: &mut Tackly, name: String) {
    expect(&world.page(&name).locator(".sync.off"))
        .to_be_visible()
        .await;
}

#[then(regex = r"^(\w+)'s app says it is live$")]
async fn says_live(world: &mut Tackly, name: String) {
    expect(&world.page(&name).locator(".sync.on"))
        .to_be_visible()
        .await;
}

// ---- runner ------------------------------------------------------------

fn build_app() -> PathBuf {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let target = workspace.join("target/ui-test");
    let status = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .args([
            "build",
            "--locked",
            "-p",
            "tackly-app",
            "--features",
            "ui-test",
            "--target-dir",
        ])
        .arg(&target)
        .current_dir(&workspace)
        .status()
        .expect("run cargo");
    assert!(status.success(), "could not build the app");
    target.join("debug/tackly-app")
}

#[tokio::main]
async fn main() {
    APP.set(build_app()).unwrap();
    Tackly::cucumber()
        .max_concurrent_scenarios(1)
        .fail_on_skipped()
        .run_and_exit("tests/features")
        .await;
}
