//! Cucumber acceptance tests. Every step is done by clicking and typing in
//! real Tackly app windows (one process per family member) against a real
//! sync server; nothing talks to the device core directly.

mod support;

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::Command,
    sync::OnceLock,
};

use cucumber::{World, given, then, when};
use support::{
    member::{Member, quote},
    server::Server,
};

static APP: OnceLock<PathBuf> = OnceLock::new();

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

    fn member(&mut self, name: &str) -> &mut Member {
        self.members
            .get_mut(name)
            .unwrap_or_else(|| panic!("{name} has not opened Tackly"))
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

    async fn go(&mut self, name: &str, tab: &str) {
        self.member(name).click(&format!("button:{tab}")).await;
    }

    async fn create_family(&mut self, name: &str, family: &str) {
        let m = self.member(name);
        m.click("button:Create a family").await;
        m.fill("field:Your name", name).await;
        m.fill("field:Family name", family).await;
        m.click("button:Create").await;
        m.wait_for_text(&format!("{family} ·")).await;
    }

    /// The owner shows an invitation, the other phone asks to join, both
    /// compare the six digits and the owner lets them in.
    async fn invite(&mut self, owner: &str, joiner: &str) {
        let mut o = self
            .members
            .remove(owner)
            .expect("owner has not opened Tackly");
        let mut j = self
            .members
            .remove(joiner)
            .expect("joiner has not opened Tackly");
        j.click("button:Join with an invitation").await;
        o.click("button:Family").await;
        o.click("button:Invite someone").await;
        let code = o
            .read(
                "the invitation code",
                "T.has('css:textarea[readonly]') && T.value('css:textarea[readonly]')",
            )
            .await;
        j.fill("field:Your name", joiner).await;
        j.fill("field:Invitation code", &code).await;
        j.click("button:Ask to join").await;
        let digits = j
            .read(
                "the confirmation digits",
                "T.has('css:.code') && T.textOf('css:.code')",
            )
            .await;
        o.wait_for(
            "the same digits on the owner's phone",
            &format!(
                "T.has('css:.code') && T.textOf('css:.code') === {}",
                quote(&digits)
            ),
        )
        .await;
        o.click("button:Yes, let them in").await;
        j.wait_for("the family home screen", "T.has('button:Tasks')")
            .await;
        o.wait_for(
            "the invitation to close",
            "!T.has('button:Yes, let them in')",
        )
        .await;
        self.members.insert(owner.to_owned(), o);
        self.members.insert(joiner.to_owned(), j);
    }

    async fn add_task(&mut self, name: &str, title: &str) {
        self.go(name, "Tasks").await;
        let m = self.member(name);
        m.click("button:New task").await;
        m.fill("field:What needs doing?", title).await;
        m.click("button:Add").await;
        m.wait_for(
            &format!("the task {title:?}"),
            &format!("T.has({})", quote(&format!("card:{title}"))),
        )
        .await;
    }

    async fn start(&mut self, name: &str, title: &str) {
        self.go(name, "Tasks").await;
        self.member(name)
            .click(&format!("card:{title} > button:Start"))
            .await;
    }

    async fn finish(&mut self, name: &str, title: &str, note: Option<&str>) {
        self.go(name, "Tasks").await;
        let m = self.member(name);
        m.click(&format!("card:{title} > button:Finish")).await;
        if let Some(note) = note {
            m.fill("field:Note (optional)", note).await;
        }
        m.click("button:Done").await;
    }

    async fn see_card(&mut self, who: &[String], title: &str, parts: impl Fn(&str) -> Vec<String>) {
        for name in who {
            let expected = parts(name);
            self.go(name, "Tasks").await;
            self.member(name).wait_for_card(title, &expected).await;
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
    world.invite(&owner, &a).await;
    world.invite(&owner, &b).await;
}

#[when(regex = r#"^(\w+) creates the family "([^"]+)"$"#)]
async fn creates_family(world: &mut Tackly, name: String, family: String) {
    world.create_family(&name, &family).await;
}

#[when(regex = r"^(\w+) invites (\w+)$")]
async fn invites(world: &mut Tackly, owner: String, joiner: String) {
    world.invite(&owner, &joiner).await;
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
    world
        .member(&name)
        .click(&format!("card:{title} > button:Reopen"))
        .await;
}

#[when(regex = r#"^(\w+) keeps (\w+)'s completion of "([^"]+)"$"#)]
async fn keeps(world: &mut Tackly, name: String, winner: String, title: String) {
    world.go(&name, "Tasks").await;
    world
        .member(&name)
        .click(&format!("card:{title} > button:Keep {winner}'s"))
        .await;
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
        let expression = format!(
            "T.has({}) && !T.cardText({}).includes('Done by')",
            quote(&format!("card:{title} > button:Start")),
            quote(&title)
        );
        world
            .member(&name)
            .wait_for(&format!("{title:?} to be open again"), &expression)
            .await;
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
    world
        .member(&name)
        .wait_for_card(&title, &["Waiting for".into(), a, b, "to decide".into()])
        .await;
    assert!(
        !world
            .member(&name)
            .has(&format!("card:{title} > button:Keep"))
            .await,
        "{name} must not be offered a choice"
    );
}

#[then(regex = r"^(.+?) sees? the members (.+) with (\w+) as head of the family$")]
async fn sees_members(world: &mut Tackly, who: String, members: String, head: String) {
    for name in names(&who) {
        world.go(&name, "Family").await;
        for member in names(&members) {
            world.member(&name).wait_for_text(&member).await;
        }
        world
            .member(&name)
            .wait_for(
                "the head of the family",
                &format!("T.text().includes('{head}') && T.text().includes('Head of the family')"),
            )
            .await;
    }
}

#[then(regex = r"^(\w+)'s app says it is offline$")]
async fn says_offline(world: &mut Tackly, name: String) {
    world
        .member(&name)
        .wait_for("the offline notice", "T.has('css:.sync.off')")
        .await;
}

#[then(regex = r"^(\w+)'s app says it is live$")]
async fn says_live(world: &mut Tackly, name: String) {
    world
        .member(&name)
        .wait_for("the live notice", "T.has('css:.sync.on')")
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
