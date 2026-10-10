//! Cucumber acceptance tests. Every step is done like a person would, with
//! Playwright-style locators in real Tackly app windows (one process per family
//! member) against a real sync server; nothing talks to the device core.
//!
//! The steps at the bottom only call the world's methods and pass the outcome
//! to `check`, which fails the scenario with the message of the first error.

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
    Failure, Outcome, check,
    member::Member,
    page::{Locator, Page, expect},
};
use tackly_testkit::Relay;

/// The app binary built with the `ui-test` feature, set once in `main`.
static APP: OnceLock<PathBuf> = OnceLock::new();

/// Delay between key presses when typing key by key.
const KEY_DELAY: Duration = Duration::from_millis(12);

#[derive(World)]
#[world(init = Self::new)]
struct Tackly {
    dir: PathBuf,
    relay: Relay,
    members: HashMap<String, Member>,
}

impl std::fmt::Debug for Tackly {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tackly")
            .field("members", &self.members.keys())
            .finish()
    }
}

/// "Mona and Mara" or "Patrick, Mona and Mara" as a list of names.
fn names(list: &str) -> Vec<String> {
    list.replace(" and ", ", ")
        .split(", ")
        .map(str::to_owned)
        .collect()
}

/// Where each member is, for the location recorded when finishing a task.
fn location(name: &str) -> &'static str {
    match name {
        "Patrick" => "52.5200,13.4050",
        "Mona" => "52.5163,13.3777",
        _ => "52.5075,13.3904",
    }
}

/// The task cards whose text contains `title`.
fn card(page: &Page, title: &str) -> Locator {
    page.locator(".card").filter_has_text(title)
}

/// How an invitation reaches the other phone.
#[derive(Clone, Copy)]
enum Via {
    Link,
    Qr,
}

impl Tackly {
    fn new() -> Outcome<Self> {
        let dir = std::env::temp_dir().join(format!("tackly-acceptance-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir)?;
        Ok(Self {
            relay: Relay::reserve(&dir)?,
            dir,
            members: HashMap::new(),
        })
    }

    fn page(&self, name: &str) -> Outcome<Page> {
        self.members
            .get(name)
            .map(|member| member.page.clone())
            .ok_or_else(|| Failure(format!("{name} has not opened Tackly")))
    }

    async fn open(&mut self, name: &str) -> Outcome<()> {
        let binary = APP.get().ok_or("the app was not built")?;
        let member =
            Member::open(name, binary, &self.dir, &self.relay.url(), location(name)).await?;
        self.members.insert(name.to_owned(), member);
        Ok(())
    }

    async fn go(&self, name: &str, tab: &str) -> Outcome<()> {
        self.page(name)?
            .get_by_role_exact("button", tab)
            .click()
            .await
    }

    async fn create_family(&self, name: &str, family: &str) -> Outcome<()> {
        let page = self.page(name)?;
        page.get_by_role_exact("button", "Create a family")
            .click()
            .await?;
        page.get_by_label("Your name")
            .press_sequentially(name, KEY_DELAY)
            .await?;
        page.get_by_label("Family name")
            .press_sequentially(family, KEY_DELAY)
            .await?;
        page.get_by_role_exact("button", "Create").click().await?;
        expect(&page.locator(".sub")).to_contain_text(family).await
    }

    /// The head of the family shows an invitation; the other phone gets it by
    /// scanning the QR code or by pasting the copied link, both phones compare
    /// the six digits, and the head lets the new member in.
    async fn invite(&self, head: &str, joiner: &str, via: Via) -> Outcome<()> {
        let (head_page, new_page) = (self.page(head)?, self.page(joiner)?);
        new_page
            .get_by_role_exact("button", "Join with an invitation")
            .click()
            .await?;
        head_page
            .get_by_role_exact("button", "Family")
            .click()
            .await?;
        head_page
            .get_by_role_exact("button", "Invite someone")
            .click()
            .await?;
        let shown = head_page
            .get_by_label("Invitation link")
            .input_value()
            .await?;
        let link = match via {
            Via::Qr => {
                let scanned = head_page.locator(".qr").decode_qr().await?;
                if scanned != shown {
                    return Err("the QR code must carry the same link that is shown".into());
                }
                scanned
            }
            Via::Link => {
                head_page
                    .get_by_role_exact("button", "Copy link")
                    .click()
                    .await?;
                expect(&head_page.locator(".snack"))
                    .to_contain_text("Link copied")
                    .await?;
                head_page.get_by_role_exact("button", "OK").click().await?;
                shown
            }
        };
        new_page
            .get_by_label("Your name")
            .press_sequentially(joiner, KEY_DELAY)
            .await?;
        new_page.get_by_label("Invitation link").fill(&link).await?;
        new_page
            .get_by_role_exact("button", "Ask to join")
            .click()
            .await?;
        let digits = new_page.locator(".code").text_content().await?;
        expect(&head_page.locator(".code"))
            .to_have_text(&digits)
            .await?;
        head_page
            .get_by_role_exact("button", "Yes, let them in")
            .click()
            .await?;
        expect(&new_page.get_by_role_exact("button", "Tasks"))
            .to_be_visible()
            .await?;
        expect(&head_page.get_by_role_exact("button", "Yes, let them in"))
            .to_be_hidden()
            .await
    }

    async fn add_task(&self, name: &str, title: &str) -> Outcome<()> {
        self.go(name, "Tasks").await?;
        let page = self.page(name)?;
        page.get_by_role_exact("button", "New task").click().await?;
        page.get_by_label("What needs doing?")
            .press_sequentially(title, KEY_DELAY)
            .await?;
        page.get_by_role_exact("button", "Add").click().await?;
        expect(&card(&page, title)).to_be_visible().await
    }

    async fn start(&self, name: &str, title: &str) -> Outcome<()> {
        self.go(name, "Tasks").await?;
        let page = self.page(name)?;
        card(&page, title)
            .get_by_role_exact("button", "Start")
            .click()
            .await
    }

    async fn finish(&self, name: &str, title: &str, note: Option<&str>) -> Outcome<()> {
        self.go(name, "Tasks").await?;
        let page = self.page(name)?;
        card(&page, title)
            .get_by_role_exact("button", "Finish")
            .click()
            .await?;
        if let Some(note) = note {
            page.get_by_label("Note (optional)")
                .press_sequentially(note, KEY_DELAY)
                .await?;
        }
        page.get_by_role_exact("button", "Done").click().await
    }

    /// Each of `who` sees the card of `title` showing every text `parts` asks for.
    async fn see_card(
        &self,
        who: &[String],
        title: &str,
        parts: impl Fn(&str) -> Vec<String>,
    ) -> Outcome<()> {
        for name in who {
            self.go(name, "Tasks").await?;
            let page = self.page(name)?;
            for part in parts(name) {
                expect(&card(&page, title)).to_contain_text(&part).await?;
            }
            expect(&card(&page, title)).to_be_visible().await?;
        }
        Ok(())
    }
}

// ---- Given / When ------------------------------------------------------------------

#[given("the sync server is running")]
#[when("the sync server is started")]
async fn server_running(world: &mut Tackly) {
    check(world.relay.start().await.map_err(Failure::from));
}

#[given("the sync server is stopped")]
#[when("the sync server is stopped")]
async fn server_stopped(world: &mut Tackly) {
    world.relay.stop();
}

#[given(regex = r"^(\w+), (\w+) and (\w+) have opened Tackly$")]
async fn opened(world: &mut Tackly, a: String, b: String, c: String) {
    for name in [a, b, c] {
        check(world.open(&name).await);
    }
}

#[given(regex = r#"^(\w+) has created the family "([^"]+)" with (\w+) and (\w+)$"#)]
async fn family_of_three(world: &mut Tackly, head: String, family: String, a: String, b: String) {
    check(world.create_family(&head, &family).await);
    check(world.invite(&head, &a, Via::Link).await);
    check(world.invite(&head, &b, Via::Link).await);
}

#[given(regex = r#"^(\w+) has created the family "([^"]+)"$"#)]
#[when(regex = r#"^(\w+) creates the family "([^"]+)"$"#)]
async fn creates_family(world: &mut Tackly, name: String, family: String) {
    check(world.create_family(&name, &family).await);
}

#[when(regex = r"^(\w+) invites (\w+)(?: by (QR code|link))?$")]
async fn invites(world: &mut Tackly, head: String, joiner: String, via: String) {
    let via = if via == "QR code" { Via::Qr } else { Via::Link };
    check(world.invite(&head, &joiner, via).await);
}

/// The titles in `"A" and "B"` or `"A", "B" and "C"`.
fn quoted_titles(list: &str) -> impl Iterator<Item = &str> {
    list.split('"').skip(1).step_by(2)
}

#[given(regex = r#"^(\w+) has added the tasks? ((?:"[^"]+"(?:, | and )?)+)$"#)]
#[when(regex = r#"^(\w+) adds the tasks? ((?:"[^"]+"(?:, | and )?)+)$"#)]
async fn adds_tasks(world: &mut Tackly, name: String, titles: String) {
    for title in quoted_titles(&titles) {
        check(world.add_task(&name, title).await);
    }
}

#[when(regex = r#"^(\w+) starts "([^"]+)"$"#)]
async fn starts(world: &mut Tackly, name: String, title: String) {
    check(world.start(&name, &title).await);
}

#[when(regex = r#"^(\w+) finishes "([^"]+)"$"#)]
async fn finishes(world: &mut Tackly, name: String, title: String) {
    check(world.finish(&name, &title, None).await);
}

#[when(regex = r#"^(\w+) finishes "([^"]+)" with the note "([^"]+)"$"#)]
async fn finishes_with_note(world: &mut Tackly, name: String, title: String, note: String) {
    check(world.finish(&name, &title, Some(&note)).await);
}

#[when(regex = r#"^(\w+) reopens "([^"]+)"$"#)]
async fn reopens(world: &mut Tackly, name: String, title: String) {
    check(world.go(&name, "Tasks").await);
    let page = check(world.page(&name));
    check(
        card(&page, &title)
            .get_by_role_exact("button", "Reopen")
            .click()
            .await,
    );
}

#[when(regex = r#"^(\w+) keeps (\w+)'s completion of "([^"]+)"$"#)]
async fn keeps(world: &mut Tackly, name: String, winner: String, title: String) {
    check(world.go(&name, "Tasks").await);
    let page = check(world.page(&name));
    let button = format!("Keep {winner}'s");
    check(
        card(&page, &title)
            .get_by_role_exact("button", &button)
            .click()
            .await,
    );
}

// ---- keyboard --------------------------------------------------------------------

#[when(regex = r"^(\w+) opens the new task form$")]
async fn opens_new_task(world: &mut Tackly, name: String) {
    check(world.go(&name, "Tasks").await);
    let page = check(world.page(&name));
    check(page.get_by_role_exact("button", "New task").click().await);
}

#[when(regex = r#"^(\w+) types "([^"]*)" into "([^"]+)" key by key$"#)]
async fn types_key_by_key(world: &mut Tackly, name: String, text: String, label: String) {
    let page = check(world.page(&name));
    check(
        page.get_by_label(&label)
            .press_sequentially(&text, KEY_DELAY)
            .await,
    );
}

#[when(regex = r#"^(\w+) presses (\w+)(?: (\d+) times)? in "([^"]+)"$"#)]
async fn presses_key(world: &mut Tackly, name: String, key: String, times: String, label: String) {
    let field = check(world.page(&name)).get_by_label(&label);
    for _ in 0..times.parse::<usize>().unwrap_or(1) {
        check(field.press(&key).await);
    }
}

#[then(regex = r#"^the field "([^"]+)" of (\w+) contains "([^"]*)"$"#)]
async fn field_contains(world: &mut Tackly, label: String, name: String, value: String) {
    let field = check(world.page(&name)).get_by_label(&label);
    check(expect(&field).to_have_value(&value).await);
}

#[then(regex = r#"^the "([^"]+)" button of (\w+) is (enabled|disabled)$"#)]
async fn button_state(world: &mut Tackly, label: String, name: String, state: String) {
    let button = check(world.page(&name)).get_by_role_exact("button", &label);
    if state == "enabled" {
        check(expect(&button).to_be_enabled().await);
    } else {
        check(expect(&button).to_be_disabled().await);
    }
}

// ---- Then --------------------------------------------------------------------------

#[given(regex = r#"^(.+?) sees? the tasks? ((?:"[^"]+"(?:, | and )?)+)$"#)]
#[then(regex = r#"^(.+?) sees? the tasks? ((?:"[^"]+"(?:, | and )?)+)$"#)]
async fn sees_tasks(world: &mut Tackly, who: String, titles: String) {
    for title in quoted_titles(&titles) {
        check(world.see_card(&names(&who), title, |_| vec![]).await);
    }
}

#[then(regex = r#"^(.+?) sees? "([^"]+)" in progress by (\w+)$"#)]
async fn sees_in_progress(world: &mut Tackly, who: String, title: String, by: String) {
    let outcome = world
        .see_card(&names(&who), &title, |viewer| {
            vec![if viewer == by {
                "You are on it".into()
            } else {
                format!("{by} is on it")
            }]
        })
        .await;
    check(outcome);
}

#[then(regex = r#"^(.+?) sees? "([^"]+)" done by (\w+)$"#)]
async fn sees_done(world: &mut Tackly, who: String, title: String, by: String) {
    let outcome = world
        .see_card(&names(&who), &title, |viewer| {
            vec![format!(
                "Done by {}",
                if viewer == by { "You" } else { &by }
            )]
        })
        .await;
    check(outcome);
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
    let outcome = world
        .see_card(&names(&who), &title, |viewer| {
            vec![
                format!("Done by {}", if viewer == by { "You" } else { &by }),
                "⏱".into(),
                format!("💬 {note}"),
                "📍 52.".into(),
            ]
        })
        .await;
    check(outcome);
}

#[then(regex = r#"^(.+?) sees? "([^"]+)" open again$"#)]
async fn sees_open(world: &mut Tackly, who: String, title: String) {
    for name in names(&who) {
        check(world.go(&name, "Tasks").await);
        let task = card(&check(world.page(&name)), &title);
        check(
            expect(&task.get_by_role_exact("button", "Start"))
                .to_be_visible()
                .await,
        );
        check(expect(&task).not().to_contain_text("Done by").await);
    }
}

#[then(regex = r#"^(.+?) sees? "([^"]+)" finished twice$"#)]
async fn sees_conflict(world: &mut Tackly, who: String, title: String) {
    let outcome = world
        .see_card(&names(&who), &title, |_| vec!["finished twice".into()])
        .await;
    check(outcome);
}

#[then(regex = r#"^(\w+) can only wait for (\w+) and (\w+) to decide "([^"]+)"$"#)]
async fn cannot_decide(world: &mut Tackly, name: String, a: String, b: String, title: String) {
    check(world.go(&name, "Tasks").await);
    let task = card(&check(world.page(&name)), &title);
    for part in ["Waiting for", &a, &b, "to decide"] {
        check(expect(&task).to_contain_text(part).await);
    }
    check(
        expect(&task.get_by_role("button", "Keep"))
            .to_have_count(0)
            .await,
    );
}

#[then(regex = r"^(.+?) sees? the members (.+) with (\w+) as head of the family$")]
async fn sees_members(world: &mut Tackly, who: String, members: String, head: String) {
    for name in names(&who) {
        check(world.go(&name, "Family").await);
        let page = check(world.page(&name));
        for member in names(&members) {
            check(expect(&card(&page, &member)).to_be_visible().await);
        }
        check(
            expect(&card(&page, &head))
                .to_contain_text("Head of the family")
                .await,
        );
    }
}

#[then(regex = r"^(\w+)'s app says it is offline$")]
async fn says_offline(world: &mut Tackly, name: String) {
    let chip = check(world.page(&name)).locator(".sync.off");
    check(expect(&chip).to_be_visible().await);
}

#[then(regex = r"^(\w+)'s app says it is live$")]
async fn says_live(world: &mut Tackly, name: String) {
    let chip = check(world.page(&name)).locator(".sync.on");
    check(expect(&chip).to_be_visible().await);
}

// ---- runner ------------------------------------------------------------------------

/// Builds the app with the `ui-test` feature in its own target directory, so
/// that the normal build is not disturbed, and returns the binary.
fn build_app() -> Outcome<PathBuf> {
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
        .status()?;
    if !status.success() {
        return Err("could not build the app".into());
    }
    Ok(target.join("debug/tackly-app"))
}

#[tokio::main]
async fn main() {
    let app = check(build_app());
    // Set only here, once.
    let _ = APP.set(app);
    Tackly::cucumber()
        .max_concurrent_scenarios(1)
        .fail_on_skipped()
        .run_and_exit("tests/features")
        .await;
}
