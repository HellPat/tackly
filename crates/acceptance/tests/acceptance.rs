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
use tackly_testkit::{FakeGeocoder, Relay};

/// The app binary built with the `ui-test` feature, set once in `main`.
static APP: OnceLock<PathBuf> = OnceLock::new();

/// Delay between key presses when typing key by key.
const KEY_DELAY: Duration = Duration::from_millis(12);

#[derive(World)]
#[world(init = Self::new)]
struct Tackly {
    dir: PathBuf,
    relay: Relay,
    geocoder: FakeGeocoder,
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

/// The task with this title, wherever its card is shown (a list, "Other", a
/// place, a problem, Today).
fn task(page: &Page, title: &str) -> Locator {
    page.get_by_test_id("main")
        .get_by_test_id("task")
        .filter_has_text(title)
}

/// The newest toast that says `text`.
fn toast(page: &Page, text: &str) -> Locator {
    page.get_by_test_id("toast").filter_has_text(text).last()
}

/// How an invitation reaches the other phone.
#[derive(Clone, Copy)]
enum Via {
    Link,
    Qr,
}

impl Tackly {
    fn new() -> Outcome<Self> {
        let dir = std::env::temp_dir().join(format!("tackly-acceptance-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir)?;
        Ok(Self {
            relay: Relay::reserve(&dir)?,
            geocoder: FakeGeocoder::start()?,
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
        let member = Member::open(
            name,
            binary,
            &self.dir,
            &self.relay.url(),
            location(name),
            &self.geocoder.url(),
        )
        .await?;
        self.members.insert(name.to_owned(), member);
        Ok(())
    }

    /// Opens a tab from the navigation at the bottom (back on its overview).
    /// The family is behind one's own picture in the top bar.
    async fn go(&self, name: &str, tab: &str) -> Outcome<()> {
        let page = self.page(name)?;
        if tab == "Family" {
            return page
                .get_by_role_exact("button", "Family and settings")
                .click()
                .await;
        }
        page.get_by_test_id("nav")
            .get_by_role_exact("button", tab)
            .click()
            .await
    }

    /// Moves this person's phone clock to `at` (minutes from now on the real clock).
    fn set_clock(&self, name: &str, at: chrono::DateTime<chrono::Local>) -> Outcome<()> {
        let seconds = (at - chrono::Local::now()).num_seconds();
        let minutes = (seconds + 59).div_euclid(60);
        std::fs::write(
            self.dir.join(name).join("clock-offset-minutes"),
            minutes.to_string(),
        )?;
        Ok(())
    }

    /// Steps into a list, group, place or person from an overview.
    async fn open_row(&self, name: &str, row: &str) -> Outcome<()> {
        self.page(name)?
            .get_by_test_id("main")
            .get_by_role("button", row)
            .first()
            .click()
            .await
    }

    /// Types into the bar at the bottom key by key and presses Enter.
    async fn add_with_bar(&self, name: &str, bar: &str, text: &str) -> Outcome<()> {
        let page = self.page(name)?;
        let field = page.get_by_label(bar);
        field.press_sequentially(text, KEY_DELAY).await?;
        field.press("Enter").await?;
        expect(&page.get_by_label(bar)).to_have_value("").await
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
        expect(&page.get_by_test_id("nav")).to_be_visible().await?;
        // The app opens on Today; tasks are added on Tasks.
        self.go(name, "Tasks").await
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
        self.go(head, "Family").await?;
        head_page
            .get_by_role_exact("button", "Invite member")
            .click()
            .await?;
        // The link itself is not shown, only the QR code and the Copy button.
        let scanned = head_page
            .get_by_label("Invitation QR code")
            .decode_qr()
            .await?;
        let link = match via {
            Via::Qr => scanned,
            Via::Link => {
                head_page
                    .get_by_role_exact("button", "Copy link")
                    .click()
                    .await?;
                expect(&toast(&head_page, "Link copied"))
                    .to_be_visible()
                    .await?;
                let copied = arboard::Clipboard::new()
                    .and_then(|mut clipboard| clipboard.get_text())
                    .map_err(|error| Failure(format!("read the clipboard: {error}")))?;
                if copied != scanned {
                    return Err("the copied link must be the one in the QR code".into());
                }
                copied
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
        let digits = new_page
            .get_by_label("Confirmation code")
            .text_content()
            .await?;
        expect(&head_page.get_by_label("Confirmation code"))
            .to_have_text(&digits)
            .await?;
        head_page
            .get_by_role_exact("button", "Yes, let them in")
            .click()
            .await?;
        expect(&new_page.get_by_test_id("nav"))
            .to_be_visible()
            .await?;
        self.go(joiner, "Tasks").await?;
        expect(&head_page.get_by_role_exact("button", "Yes, let them in"))
            .to_be_hidden()
            .await?;
        // Back where tasks are added, like a person would go.
        self.go(head, "Tasks").await
    }

    async fn add_task(&self, name: &str, title: &str) -> Outcome<()> {
        // Tasks are added where the bar is; from Today or Problems, that is Tasks.
        if self.page(name)?.get_by_label("Add a task").count().await == 0 {
            self.go(name, "Tasks").await?;
        }
        self.add_with_bar(name, "Add a task", title).await?;
        // The card may be hidden by the filter; the toast says it was added.
        expect(&toast(&self.page(name)?, &format!("Added {title}")))
            .to_be_visible()
            .await
    }

    /// Opens a task in focus; there `Start`, `Pause` or `Finish`.
    async fn in_focus(&self, name: &str, title: &str, button: &str) -> Outcome<()> {
        let page = self.page(name)?;
        task(&page, title)
            .get_by_role("button", title)
            .first()
            .click()
            .await?;
        page.get_by_test_id("task-focus")
            .get_by_role_exact("button", button)
            .click()
            .await?;
        if button != "Finish" {
            page.get_by_test_id("task-focus")
                .get_by_role_exact("button", "Back")
                .click()
                .await?;
        }
        Ok(())
    }

    /// Ticks the circle: done, the card slides away.
    async fn tick(&self, name: &str, title: &str) -> Outcome<()> {
        let page = self.page(name)?;
        page.get_by_role_exact("checkbox", &format!("Done: {title}"))
            .click()
            .await?;
        expect(&task(&page, title)).to_be_hidden().await
    }

    async fn assign(&self, name: &str, title: &str, to: &str) -> Outcome<()> {
        let page = self.page(name)?;
        page.get_by_role_exact("button", &format!("Assign {title}"))
            .click()
            .await?;
        let who = if to == name { "Me" } else { to };
        page.get_by_test_id("sheet")
            .get_by_role_exact("button", who)
            .click()
            .await?;
        expect(&page.get_by_test_id("sheet")).to_be_hidden().await
    }

    async fn filter(&self, name: &str, filter: &str) -> Outcome<()> {
        self.page(name)?
            .get_by_test_id("filter")
            .get_by_role("button", filter)
            .click()
            .await
    }

    async fn new_list(&self, name: &str, list: &str) -> Outcome<()> {
        let page = self.page(name)?;
        self.go(name, "Tasks").await?;
        page.get_by_role_exact("button", "New list").click().await?;
        page.get_by_label("List name")
            .press_sequentially(list, KEY_DELAY)
            .await?;
        page.get_by_role_exact("button", "Create").click().await?;
        expect(&page.get_by_test_id("main").get_by_role("button", list))
            .to_be_visible()
            .await
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

fn quoted(list: &str) -> impl Iterator<Item = &str> {
    list.split('"').skip(1).step_by(2)
}

#[given(regex = r#"^(\w+) has added the tasks? ((?:"[^"]+"(?:, | and )?)+)$"#)]
#[when(regex = r#"^(\w+) adds the tasks? ((?:"[^"]+"(?:, | and )?)+)$"#)]
async fn adds_tasks(world: &mut Tackly, name: String, titles: String) {
    for title in quoted(&titles) {
        check(world.add_task(&name, title).await);
    }
}

#[when(regex = r#"^(\w+) opens the tab "([^"]+)"$"#)]
async fn opens_tab(world: &mut Tackly, name: String, tab: String) {
    check(world.go(&name, &tab).await);
}

#[when(regex = r#"^(\w+) opens "([^"]+)"$"#)]
async fn opens_row(world: &mut Tackly, name: String, row: String) {
    check(world.open_row(&name, &row).await);
}

#[when(regex = r"^(\w+) goes back$")]
async fn goes_back(world: &mut Tackly, name: String) {
    check(
        check(world.page(&name))
            .get_by_role_exact("button", "Back")
            .first()
            .click()
            .await,
    );
}

#[when(regex = r#"^(\w+) (starts|pauses|finishes) "([^"]+)"$"#)]
async fn works_on(world: &mut Tackly, name: String, action: String, title: String) {
    let button = match action.as_str() {
        "starts" => "Start",
        "pauses" => "Pause",
        _ => "Finish",
    };
    check(world.in_focus(&name, &title, button).await);
}

#[when(regex = r#"^(\w+) ticks "([^"]+)" off$"#)]
async fn ticks(world: &mut Tackly, name: String, title: String) {
    check(world.tick(&name, &title).await);
}

#[when(regex = r#"^(\w+) undoes "([^"]+)"$"#)]
async fn undoes(world: &mut Tackly, name: String, toast_text: String) {
    let page = check(world.page(&name));
    check(
        toast(&page, &toast_text)
            .get_by_role_exact("button", "Undo")
            .click()
            .await,
    );
}

#[when(regex = r#"^(\w+) gives "([^"]+)" to (\w+)$"#)]
async fn gives(world: &mut Tackly, name: String, title: String, to: String) {
    check(world.assign(&name, &title, &to).await);
}

#[given(regex = r#"^(\w+) shows (Mine|Unassigned|All)$"#)]
#[when(regex = r#"^(\w+) shows (Mine|Unassigned|All)$"#)]
async fn shows(world: &mut Tackly, name: String, filter: String) {
    check(world.filter(&name, &filter).await);
}

#[when(regex = r#"^(\w+) creates the list "([^"]+)"$"#)]
async fn creates_list(world: &mut Tackly, name: String, list: String) {
    check(world.new_list(&name, &list).await);
}

#[when(regex = r#"^(\w+) renames the list to "([^"]+)"$"#)]
async fn renames_list(world: &mut Tackly, name: String, list: String) {
    let page = check(world.page(&name));
    check(page.get_by_role_exact("button", "Edit").click().await);
    check(page.get_by_label("List name").fill(&list).await);
    check(page.get_by_role_exact("button", "Save").click().await);
}

#[when(regex = r#"^(\w+) deletes the list$"#)]
async fn deletes_list(world: &mut Tackly, name: String) {
    let page = check(world.page(&name));
    check(page.get_by_role_exact("button", "Edit").click().await);
    check(
        page.get_by_role_exact("button", "Delete list")
            .click()
            .await,
    );
}

#[when(regex = r#"^(\w+) adds the group "([^"]+)"$"#)]
async fn adds_group(world: &mut Tackly, name: String, group: String) {
    check(world.go(&name, "Places").await);
    check(world.add_with_bar(&name, "Add a group", &group).await);
}

#[when(regex = r#"^(\w+) adds the place "([^"]+)" by picking the address "([^"]+)"$"#)]
async fn adds_place_picking(world: &mut Tackly, name: String, typed: String, address: String) {
    let page = check(world.page(&name));
    check(
        page.get_by_label("Add a place")
            .press_sequentially(&typed, KEY_DELAY)
            .await,
    );
    check(
        page.get_by_test_id("addresses")
            .get_by_role("button", &address)
            .click()
            .await,
    );
}

#[when(regex = r#"^(\w+) adds the place "([^"]+)"$"#)]
async fn adds_place(world: &mut Tackly, name: String, place: String) {
    check(world.add_with_bar(&name, "Add a place", &place).await);
}

#[when(regex = r#"^(\w+) adds the location "([^"]+)"$"#)]
async fn adds_location(world: &mut Tackly, name: String, location: String) {
    check(world.add_with_bar(&name, "Add a location", &location).await);
}

#[when(regex = r#"^(\w+) edits the place$"#)]
async fn edits_place(world: &mut Tackly, name: String) {
    check(
        check(world.page(&name))
            .get_by_role_exact("button", "Edit")
            .click()
            .await,
    );
}

#[when(regex = r#"^(\w+) opens (?:her|his|their) settings$"#)]
async fn opens_settings(world: &mut Tackly, name: String) {
    check(world.go(&name, "Family").await);
    check(world.open_row(&name, &name).await);
}

#[when(regex = r#"^(\w+) changes (?:her|his|their) name to "([^"]+)"$"#)]
async fn changes_name(world: &mut Tackly, name: String, new_name: String) {
    let field = check(world.page(&name)).get_by_label("Your name");
    check(field.fill(&new_name).await);
    check(field.press("Enter").await);
}

#[when(regex = r#"^(\w+) picks the picture "([^"]+)"$"#)]
async fn picks_picture(world: &mut Tackly, name: String, picture: String) {
    check(
        check(world.page(&name))
            .get_by_role_exact("radio", &picture)
            .click()
            .await,
    );
}

#[when(regex = r#"^(\w+) chooses a photo as (?:her|his|their) picture$"#)]
async fn chooses_photo(world: &mut Tackly, name: String) {
    let photo = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/photo.jpg");
    let page = check(world.page(&name));
    check(
        page.get_by_label("Choose a photo")
            .set_input_files(&photo, "image/jpeg")
            .await,
    );
}

#[when(regex = r#"^(\w+) picks the color scheme "([^"]+)"$"#)]
async fn picks_scheme(world: &mut Tackly, name: String, scheme: String) {
    check(
        check(world.page(&name))
            .get_by_role_exact("radio", &scheme)
            .click()
            .await,
    );
}

// ---- keyboard --------------------------------------------------------------------

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

#[then(regex = r#"^(\w+) (sees|does not see) the "([^"]+)" button$"#)]
async fn sees_button(world: &mut Tackly, name: String, seen: String, label: String) {
    let button = check(world.page(&name)).get_by_role_exact("button", &label);
    check(if seen == "sees" {
        expect(&button).to_be_visible().await
    } else {
        expect(&button).to_be_hidden().await
    });
}

// ---- Then ------------------------------------------------------------------------

#[given(regex = r#"^(\w+(?:, \w+)*(?: and \w+)?) sees? the tasks? ((?:"[^"]+"(?:, | and )?)+)$"#)]
#[then(regex = r#"^(\w+(?:, \w+)*(?: and \w+)?) sees? the tasks? ((?:"[^"]+"(?:, | and )?)+)$"#)]
async fn sees_tasks(world: &mut Tackly, who: String, titles: String) {
    for name in names(&who) {
        let page = check(world.page(&name));
        for title in quoted(&titles) {
            check(expect(&task(&page, title)).to_be_visible().await);
        }
    }
}

#[then(regex = r#"^(\w+(?:, \w+)*(?: and \w+)?) (?:does not|do not|no longer) sees? "([^"]+)"$"#)]
async fn does_not_see(world: &mut Tackly, who: String, title: String) {
    for name in names(&who) {
        check(
            expect(&task(&check(world.page(&name)), &title))
                .to_be_hidden()
                .await,
        );
    }
}

#[then(
    regex = r#"^(\w+(?:, \w+)*(?: and \w+)?) sees? that (\w+) (is working on|paused|picked|has) "([^"]+)"$"#
)]
async fn sees_someone_on(world: &mut Tackly, who: String, by: String, how: String, title: String) {
    for name in names(&who) {
        let page = check(world.page(&name));
        let words = match how.as_str() {
            "is working on" if by == name => "You’re working on it".to_owned(),
            "is working on" => format!("{by} is working on it"),
            "paused" => format!("{by} paused it"),
            "picked" => format!("{by} picked it"),
            _ if by == name => "Assigned to you".to_owned(),
            _ => format!("Assigned to {by}"),
        };
        check(expect(&task(&page, &title)).to_contain_text(&words).await);
    }
}

#[then(regex = r#"^(\w+) cannot tick "([^"]+)" off$"#)]
async fn cannot_tick(world: &mut Tackly, name: String, title: String) {
    let page = check(world.page(&name));
    check(
        expect(&page.get_by_role_exact("checkbox", &format!("Done: {title}")))
            .to_be_hidden()
            .await,
    );
}

#[then(regex = r#"^(\w+) sees (\d+) in (Mine|Unassigned|All)$"#)]
async fn sees_count(world: &mut Tackly, name: String, count: String, filter: String) {
    let chip = check(world.page(&name))
        .get_by_test_id("filter")
        .get_by_role_exact("button", &format!("{filter} ({count})"));
    check(expect(&chip).to_be_visible().await);
}

#[then(regex = r#"^(\w+(?:, \w+)*(?: and \w+)?) sees? the list "([^"]+)"(?: with "([^"]+)")?$"#)]
async fn sees_list(world: &mut Tackly, who: String, list: String, detail: String) {
    for name in names(&who) {
        check(world.go(&name, "Tasks").await);
        let row = check(world.page(&name))
            .get_by_test_id("main")
            .get_by_role("button", &list);
        check(expect(&row).to_be_visible().await);
        if !detail.is_empty() {
            check(expect(&row).to_contain_text(&detail).await);
        }
    }
}

#[then(regex = r#"^(\w+(?:, \w+)*(?: and \w+)?) (?:does not|do not) see the list "([^"]+)"$"#)]
async fn does_not_see_list(world: &mut Tackly, who: String, list: String) {
    for name in names(&who) {
        check(world.go(&name, "Tasks").await);
        check(
            expect(
                &check(world.page(&name))
                    .get_by_test_id("main")
                    .get_by_role("button", &list),
            )
            .to_be_hidden()
            .await,
        );
    }
}

#[then(regex = r#"^(\w+(?:, \w+)*(?: and \w+)?) sees? "([^"]+)" with "([^"]+)"$"#)]
async fn sees_row(world: &mut Tackly, who: String, row: String, detail: String) {
    for name in names(&who) {
        let row = check(world.page(&name))
            .get_by_test_id("main")
            .get_by_role("button", &row);
        check(expect(&row).to_contain_text(&detail).await);
    }
}

#[then(regex = r#"^(\w+) sees the text "([^"]+)"$"#)]
async fn sees_text(world: &mut Tackly, name: String, text: String) {
    check(
        expect(
            &check(world.page(&name))
                .get_by_test_id("main")
                .get_by_text(&text),
        )
        .to_be_visible()
        .await,
    );
}

#[then(regex = r"^(\w+(?:, \w+)*(?: and \w+)?) sees? the members (.+)$")]
async fn sees_members(world: &mut Tackly, who: String, members: String) {
    for name in names(&who) {
        check(world.go(&name, "Family").await);
        let page = check(world.page(&name));
        for member in names(&members) {
            check(
                expect(&page.get_by_test_id("main").get_by_role("button", &member))
                    .to_be_visible()
                    .await,
            );
        }
    }
}

#[then(regex = r#"^(\w+)'s settings show the photo, with no icon picked$"#)]
async fn settings_show_photo(world: &mut Tackly, name: String) {
    let page = check(world.page(&name));
    check(
        expect(&page.get_by_test_id("main").get_by_test_id("photo"))
            .to_be_visible()
            .await,
    );
    check(
        expect(
            &page
                .get_by_test_id("picture")
                .get_by_role_checked("radio", None),
        )
        .to_have_count(0)
        .await,
    );
}

#[then(regex = r#"^(\w+(?:, \w+)*(?: and \w+)?) sees? (\w+)'s photo$"#)]
async fn sees_photo(world: &mut Tackly, who: String, member: String) {
    for name in names(&who) {
        check(world.go(&name, "Family").await);
        let row = check(world.page(&name))
            .get_by_test_id("main")
            .get_by_role("button", &member);
        check(expect(&row.get_by_test_id("photo")).to_be_visible().await);
    }
}

#[then(regex = r#"^(\w+)'s color scheme is "([^"]+)"$"#)]
async fn scheme_is(world: &mut Tackly, name: String, scheme: String) {
    let radio = check(world.page(&name)).get_by_role_checked("radio", Some(&scheme));
    check(expect(&radio).to_be_visible().await);
}

#[then(regex = r"^(\w+)'s app says it is offline$")]
async fn says_offline(world: &mut Tackly, name: String) {
    check(
        expect(
            &check(world.page(&name))
                .get_by_test_id("header")
                .get_by_text("Offline"),
        )
        .to_be_visible()
        .await,
    );
}

#[then(regex = r"^(\w+)'s app says it is live$")]
async fn says_live(world: &mut Tackly, name: String) {
    check(
        expect(
            &check(world.page(&name))
                .get_by_test_id("header")
                .get_by_text("Offline"),
        )
        .to_be_hidden()
        .await,
    );
}

// ---- problems ----------------------------------------------------------------------

/// Something’s wrong, from the Problems tab.
async fn something_wrong(world: &Tackly, name: &str, typed: &str) -> Outcome<Page> {
    let page = world.page(name)?;
    world.go(name, "Problems").await?;
    page.get_by_role_exact("button", "Something’s wrong")
        .click()
        .await?;
    page.get_by_label("What’s wrong?")
        .press_sequentially(typed, KEY_DELAY)
        .await?;
    Ok(page)
}

#[given(regex = r#"^(\w+) has noticed "([^"]+)"$"#)]
#[when(regex = r#"^(\w+) notices "([^"]+)"$"#)]
async fn notices(world: &mut Tackly, name: String, problem: String) {
    let page = check(something_wrong(world, &name, &problem).await);
    check(
        page.get_by_role_exact("button", &format!("Notice {problem}"))
            .click()
            .await,
    );
    check(
        expect(&toast(&page, &format!("Noticed: {problem}")))
            .to_be_visible()
            .await,
    );
}

#[given(regex = r#"^(\w+) has raised "([^"]+)" for (\w+)$"#)]
#[when(regex = r#"^(\w+) raises "([^"]+)" for (\w+)$"#)]
async fn raises(world: &mut Tackly, name: String, problem: String, to: String) {
    let page = check(something_wrong(world, &name, &problem).await);
    check(
        page.get_by_role_exact("button", &format!("Raise {problem}"))
            .click()
            .await,
    );
    check(
        page.get_by_test_id("sheet")
            .get_by_role_exact("button", &to)
            .click()
            .await,
    );
    check(
        expect(&toast(&page, &format!("Raised for {to}: {problem}")))
            .to_be_visible()
            .await,
    );
}

#[when(
    regex = r#"^(\w+) makes the new problem "([^"]+)" with the solution "([^"]+)" and notices it$"#
)]
async fn makes_problem(world: &mut Tackly, name: String, problem: String, solution: String) {
    let page = check(something_wrong(world, &name, &problem).await);
    check(
        page.get_by_role_exact("button", &format!("New problem “{problem}”"))
            .click()
            .await,
    );
    let field = page.get_by_label("Add a solution");
    check(field.press_sequentially(&solution, KEY_DELAY).await);
    check(field.press("Enter").await);
    check(
        expect(&page.get_by_test_id("solutions").get_by_text(&solution))
            .to_be_visible()
            .await,
    );
    check(page.get_by_role_exact("button", "Notice").click().await);
    check(
        expect(&toast(&page, &format!("Noticed: {problem}")))
            .to_be_visible()
            .await,
    );
}

#[when(regex = r#"^(\w+) opens the problem "([^"]+)"$"#)]
async fn opens_problem(world: &mut Tackly, name: String, problem: String) {
    check(world.go(&name, "Problems").await);
    check(world.open_row(&name, &problem).await);
}

#[when(regex = r#"^(\w+) gives the problem "([^"]+)" to (\w+)$"#)]
async fn gives_problem(world: &mut Tackly, name: String, problem: String, to: String) {
    let page = check(world.page(&name));
    check(world.go(&name, "Problems").await);
    check(world.open_row(&name, &problem).await);
    check(
        page.get_by_role_exact("button", "Edit problem")
            .click()
            .await,
    );
    check(
        page.get_by_role_exact("button", &format!("Assign {problem}"))
            .click()
            .await,
    );
    let who = if to == name { "Me" } else { to.as_str() };
    check(
        page.get_by_test_id("sheet")
            .get_by_role_exact("button", who)
            .click()
            .await,
    );
}

#[when(regex = r#"^(\w+) resolves it$"#)]
async fn resolves(world: &mut Tackly, name: String) {
    check(
        check(world.page(&name))
            .get_by_role_exact("button", "Resolve")
            .click()
            .await,
    );
}

#[then(regex = r#"^(\w+(?:, \w+)*(?: and \w+)?) sees? the problem "([^"]+)"$"#)]
async fn sees_problem(world: &mut Tackly, who: String, problem: String) {
    for name in names(&who) {
        check(world.go(&name, "Problems").await);
        let row = check(world.page(&name))
            .get_by_test_id("main")
            .get_by_role("button", &problem);
        check(expect(&row).to_be_visible().await);
        check(expect(&row).to_contain_text("Open since just now").await);
    }
}

#[then(
    regex = r#"^(\w+(?:, \w+)*(?: and \w+)?) (?:does not|do not|no longer) sees? the problem "([^"]+)"$"#
)]
async fn does_not_see_problem(world: &mut Tackly, who: String, problem: String) {
    for name in names(&who) {
        check(world.go(&name, "Problems").await);
        let row = check(world.page(&name))
            .get_by_test_id("main")
            .get_by_role("button", &problem);
        check(expect(&row).to_be_hidden().await);
    }
}

#[then(regex = r#"^(\w+)'s Problems tab shows (\d+)$"#)]
async fn badge_shows(world: &mut Tackly, name: String, count: String) {
    // The number is hidden from screen readers (the tab says it); its value is on the badge.
    let badge = check(world.page(&name))
        .get_by_test_id("nav")
        .get_by_test_id("badge");
    check(if count == "0" {
        expect(&badge).to_have_count(0).await
    } else {
        expect(&badge).to_have_attribute("data-count", &count).await
    });
}

// ---- planning: Upcoming and Today ----------------------------------------------------

#[given(regex = r#"^(\w+) plans the next day at "(\d\d:\d\d)"$"#)]
#[when(regex = r#"^(\w+) sets (?:her|his|their) planning time to "(\d\d:\d\d)"$"#)]
async fn sets_plan_time(world: &mut Tackly, name: String, time: String) {
    let page = check(world.page(&name));
    check(world.go(&name, "Family").await);
    check(world.open_row(&name, &name).await);
    check(page.get_by_label("Planning time").fill(&time).await);
}

#[given(regex = r#"^it is (\d\d):(\d\d) for (\w+)$"#)]
#[when(regex = r#"^it is (\d\d):(\d\d) for (\w+)$"#)]
async fn it_is(world: &mut Tackly, hour: String, minute: String, name: String) {
    let today = chrono::Local::now().date_naive();
    let at = today
        .and_hms_opt(hour.parse().unwrap_or(0), minute.parse().unwrap_or(0), 0)
        .and_then(|time| time.and_local_timezone(chrono::Local).earliest());
    check(world.set_clock(
        &name,
        check(at.ok_or_else(|| Failure("no such time".into()))),
    ));
}

#[when(regex = r#"^the next morning comes for (\w+)$"#)]
async fn next_morning(world: &mut Tackly, name: String) {
    let tomorrow = chrono::Local::now().date_naive() + chrono::Duration::days(1);
    let at = tomorrow
        .and_hms_opt(8, 0, 0)
        .and_then(|time| time.and_local_timezone(chrono::Local).earliest());
    check(world.set_clock(
        &name,
        check(at.ok_or_else(|| Failure("no such time".into()))),
    ));
}

fn upcoming(page: &Page) -> Locator {
    page.get_by_test_id("upcoming")
}

#[then(regex = r#"^Upcoming (opens|is closed) for (\w+)$"#)]
async fn upcoming_shown(world: &mut Tackly, how: String, name: String) {
    let page = check(world.page(&name));
    check(if how == "opens" {
        expect(&upcoming(&page).get_by_role_exact("heading", "Upcoming"))
            .to_be_visible()
            .await
    } else {
        expect(&upcoming(&page)).to_have_count(0).await
    });
}

#[when(regex = r#"^(\w+) moves "([^"]+)" into the focus$"#)]
async fn moves_into_focus(world: &mut Tackly, name: String, title: String) {
    check(
        upcoming(&check(world.page(&name)))
            .get_by_role_exact("button", &format!("Move {title} into focus"))
            .click()
            .await,
    );
}

#[when(regex = r#"^(\w+) moves "([^"]+)" back$"#)]
async fn moves_back(world: &mut Tackly, name: String, title: String) {
    check(
        upcoming(&check(world.page(&name)))
            .get_by_role_exact("button", &format!("Move {title} back"))
            .click()
            .await,
    );
}

/// The row of `title` inside `root`, and the first row of its focus.
fn row_in(root: &Locator, title: &str) -> Locator {
    root.get_by_test_id("task").filter_has_text(title)
}

#[when(regex = r#"^(\w+) drags "([^"]+)" to the top of the focus$"#)]
async fn drags_to_focus_top(world: &mut Tackly, name: String, title: String) {
    let root = upcoming(&check(world.page(&name)));
    let handle = row_in(&root, &title).get_by_test_id("drag-handle");
    let first = root.get_by_test_id("focus").get_by_test_id("task").first();
    check(handle.drag_to(&first, "top").await);
}

#[when(regex = r#"^(\w+) drags "([^"]+)" to the top of (?:her|his|their) priorities$"#)]
async fn drags_to_priorities_top(world: &mut Tackly, name: String, title: String) {
    let root = check(world.page(&name)).get_by_test_id("main");
    let handle = row_in(&root, &title).get_by_test_id("drag-handle");
    let first = root
        .get_by_test_id("priorities")
        .get_by_test_id("task")
        .first();
    check(handle.drag_to(&first, "top").await);
}

#[when(regex = r#"^(\w+) steps into the problem "([^"]+)"$"#)]
async fn steps_into(world: &mut Tackly, name: String, problem: String) {
    check(
        upcoming(&check(world.page(&name)))
            .get_by_test_id("pool")
            .get_by_role("button", &problem)
            .click()
            .await,
    );
}

#[when(regex = r#"^(\w+) goes back to all problems$"#)]
async fn back_to_problems(world: &mut Tackly, name: String) {
    check(
        upcoming(&check(world.page(&name)))
            .get_by_role_exact("button", "All problems")
            .click()
            .await,
    );
}

#[when(regex = r#"^(\w+) swipes "([^"]+)" away$"#)]
async fn swipes_away(world: &mut Tackly, name: String, title: String) {
    let root = upcoming(&check(world.page(&name)));
    check(row_in(&root, &title).swipe_right().await);
    check(expect(&row_in(&root, &title)).to_be_hidden().await);
}

#[when(regex = r#"^(\w+) (saves the plan|plans later)$"#)]
async fn closes_upcoming(world: &mut Tackly, name: String, how: String) {
    let button = if how == "saves the plan" {
        "Save plan"
    } else {
        "Plan later"
    };
    let page = check(world.page(&name));
    check(
        upcoming(&page)
            .get_by_role_exact("button", button)
            .click()
            .await,
    );
    check(expect(&upcoming(&page)).to_have_count(0).await);
}

#[then(regex = r#"^Upcoming of (\w+) (offers|does not offer) "([^"]+)"$"#)]
async fn upcoming_offers(world: &mut Tackly, name: String, how: String, title: String) {
    let row = upcoming(&check(world.page(&name)))
        .get_by_test_id("pool")
        .get_by_text(&title);
    check(if how == "offers" {
        expect(&row).to_be_visible().await
    } else {
        expect(&row).to_be_hidden().await
    });
}

/// The tasks of a list (the focus, the priorities), in order, are these.
async fn in_order(root: &Locator, list: &str, titles: &str) -> Outcome<()> {
    let rows = root.get_by_test_id(list).get_by_test_id("task");
    let wanted: Vec<&str> = quoted(titles).collect();
    expect(&rows).to_have_count(wanted.len()).await?;
    for (i, title) in wanted.iter().enumerate() {
        expect(&rows.nth(i)).to_contain_text(title).await?;
    }
    Ok(())
}

#[then(regex = r#"^the focus of (\w+) is ((?:"[^"]+"(?:, | and )?)+)$"#)]
async fn focus_is(world: &mut Tackly, name: String, titles: String) {
    check(in_order(&upcoming(&check(world.page(&name))), "focus", &titles).await);
}

#[then(regex = r#"^(\w+)'s priorities are ((?:"[^"]+"(?:, | and )?)+)$"#)]
async fn priorities_are(world: &mut Tackly, name: String, titles: String) {
    check(world.go(&name, "Today").await);
    check(
        in_order(
            &check(world.page(&name)).get_by_test_id("main"),
            "priorities",
            &titles,
        )
        .await,
    );
}

#[then(regex = r#"^a kitten celebrates with (\w+)$"#)]
async fn kitten(world: &mut Tackly, name: String) {
    let page = check(world.page(&name));
    check(
        expect(&page.get_by_test_id("celebration").get_by_text("done!"))
            .to_be_visible()
            .await,
    );
}

#[when(regex = r#"^(\w+) picks more$"#)]
async fn picks_more(world: &mut Tackly, name: String) {
    check(
        check(world.page(&name))
            .get_by_role_exact("button", "Pick more")
            .click()
            .await,
    );
}

#[when(regex = r#"^(\w+) takes "([^"]+)"$"#)]
async fn takes(world: &mut Tackly, name: String, title: String) {
    check(
        check(world.page(&name))
            .get_by_role_exact("button", &format!("Take {title}"))
            .click()
            .await,
    );
}

#[then(regex = r#"^(\w+) is told "([^"]+)"$"#)]
async fn is_told(world: &mut Tackly, name: String, text: String) {
    check(
        expect(&toast(&check(world.page(&name)), &text))
            .to_be_visible()
            .await,
    );
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
    Ok(target.join("debug/tackly"))
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
