//! The vocabulary the aggregate and projection tests are written in: a family
//! of three, a few lists, places and tasks with fixed IDs, and short ways to
//! say "Patrick did this to that".

#![allow(dead_code)] // each test file uses part of it
#![allow(clippy::expect_used)] // a test fails by panicking

use chrono::{DateTime, TimeZone, Utc};
use cqrs_es::test::TestFramework;
use tackly_domain::{
    CommandContext, DomainEvent, Family, FamilyEvent, Focus, GeoPoint, IconName, Name,
    PlaceLocation, Solution,
};
use uuid::Uuid;

pub const PATRICK: Uuid = Uuid::from_u128(0x1);
pub const MONA: Uuid = Uuid::from_u128(0x2);
pub const MARA: Uuid = Uuid::from_u128(0x3);
pub const AGENT: Uuid = Uuid::from_u128(0x4);

pub const SMITHS: Uuid = Uuid::from_u128(0x10);
pub const SHOPPING: Uuid = Uuid::from_u128(0x20);
pub const GROCERY_STORE: Uuid = Uuid::from_u128(0x30);
pub const LIDL: Uuid = Uuid::from_u128(0x40);
pub const MILK: Uuid = Uuid::from_u128(0x50);
pub const DISHES: Uuid = Uuid::from_u128(0x51);

pub const OUT_OF_FOOD: Uuid = Uuid::from_u128(0x60);
pub const GO_SHOPPING: Uuid = Uuid::from_u128(0x61);
pub const COOK_PASTA: Uuid = Uuid::from_u128(0x62);
/// The steps made when "Out of food" is noticed.
pub const SHOPPING_TRIP: Uuid = Uuid::from_u128(0x70);
pub const PASTA: Uuid = Uuid::from_u128(0x71);

/// Every event in these tests has this ID and happens at 9:00 (plus minutes).
pub const EVENT: Uuid = Uuid::from_u128(0xE);

/// A day's plan (the tests only use sensible ones).
pub fn focus(tasks: &[Uuid]) -> Focus {
    Focus::new(tasks.iter().copied()).expect("a plan")
}

/// An icon name (the tests only use good ones).
pub fn icon(text: &str) -> IconName {
    text.parse().expect("an icon name")
}

/// A name or title (the tests only use good ones).
pub fn named(text: &str) -> Name {
    text.parse().expect("a name")
}

pub fn nine_o_clock() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 11, 9, 0, 0)
        .single()
        .unwrap_or_default()
}

/// The aggregate under test, acting as `who`.
pub fn as_member(who: Uuid) -> TestFramework<Family> {
    TestFramework::with(CommandContext::fixed(who, nine_o_clock(), EVENT))
}

/// `who` did `event` to `subject`, at 9:00.
pub fn by(who: Uuid, subject: Uuid, event: DomainEvent) -> FamilyEvent {
    by_at(who, subject, 0, event)
}

/// `who` did `event` to `subject`, `minutes` after 9:00.
pub fn by_at(who: Uuid, subject: Uuid, minutes: i64, event: DomainEvent) -> FamilyEvent {
    FamilyEvent {
        id: EVENT,
        subject_id: subject,
        origin_device_id: who,
        occurred_at: nine_o_clock() + chrono::Duration::minutes(minutes),
        event,
    }
}

// ---- common situations ----------------------------------------------------------------

/// Patrick's family, with Mona and Mara in it.
pub fn the_smiths() -> Vec<FamilyEvent> {
    vec![
        by(
            PATRICK,
            SMITHS,
            DomainEvent::FamilyCreated {
                name: named("The Smiths"),
                owner_name: named("Patrick"),
            },
        ),
        by(
            MONA,
            SMITHS,
            DomainEvent::MemberJoined {
                name: named("Mona"),
            },
        ),
        by(
            MARA,
            SMITHS,
            DomainEvent::MemberJoined {
                name: named("Mara"),
            },
        ),
    ]
}

pub fn shopping_list() -> FamilyEvent {
    by(
        PATRICK,
        SHOPPING,
        DomainEvent::ListCreated {
            name: named("Shopping"),
        },
    )
}

pub fn lidl_in_the_grocery_store() -> Vec<FamilyEvent> {
    vec![
        by(
            PATRICK,
            GROCERY_STORE,
            DomainEvent::PlaceGroupCreated {
                name: named("Grocery Store"),
            },
        ),
        by(
            PATRICK,
            LIDL,
            DomainEvent::PlaceCreated {
                group_id: GROCERY_STORE,
                name: named("LIDL"),
                location: winnenden(),
            },
        ),
    ]
}

pub fn winnenden() -> PlaceLocation {
    PlaceLocation {
        id: Uuid::from_u128(0x41),
        name: named("LIDL Winnenden"),
        address: None,
        point: None,
    }
}

pub fn backnang() -> PlaceLocation {
    PlaceLocation {
        id: Uuid::from_u128(0x42),
        name: named("LIDL Backnang"),
        address: None,
        point: None,
    }
}

/// A task created by Patrick, in "Other".
/// Where the GPS says the phone is: in Winnenden.
pub fn here() -> GeoPoint {
    GeoPoint::new(48.8752, 9.3775, Some(12.0)).expect("coordinates")
}

pub fn task(id: Uuid, title: &str) -> FamilyEvent {
    by(
        PATRICK,
        id,
        DomainEvent::TaskCreated {
            list_id: None,
            title: named(title),
            place_ids: vec![],
            problem_id: None,
        },
    )
}

/// "Out of food": go shopping, cook pasta.
pub fn out_of_food() -> Vec<FamilyEvent> {
    vec![
        by(
            PATRICK,
            OUT_OF_FOOD,
            DomainEvent::ProblemCreated {
                title: named("Out of food"),
                icon: icon("kitchen"),
            },
        ),
        by(
            PATRICK,
            OUT_OF_FOOD,
            DomainEvent::ProblemSolutionAdded {
                solution: go_shopping(),
            },
        ),
        by(
            PATRICK,
            OUT_OF_FOOD,
            DomainEvent::ProblemSolutionAdded {
                solution: cook_pasta(),
            },
        ),
    ]
}

pub fn go_shopping() -> Solution {
    Solution {
        id: GO_SHOPPING,
        title: named("Go shopping"),
    }
}

pub fn cook_pasta() -> Solution {
    Solution {
        id: COOK_PASTA,
        title: named("Cook pasta"),
    }
}

/// A step of a problem: its task, given to whoever has the problem.
pub fn step(who: Uuid, problem: Uuid, id: Uuid, title: &str, holder: Uuid) -> [FamilyEvent; 2] {
    [
        by(
            who,
            id,
            DomainEvent::TaskCreated {
                list_id: None,
                title: named(title),
                place_ids: vec![],
                problem_id: Some(problem),
            },
        ),
        by(
            who,
            id,
            DomainEvent::TaskAssigned {
                member_id: Some(holder),
            },
        ),
    ]
}

/// Patrick noticed "Out of food" for `holder`: the problem is open, with its two steps.
pub fn out_of_food_noticed_for(holder: Uuid) -> Vec<FamilyEvent> {
    let mut events = vec![by(
        PATRICK,
        OUT_OF_FOOD,
        DomainEvent::ProblemNoticed { member_id: holder },
    )];
    events.extend(step(
        PATRICK,
        OUT_OF_FOOD,
        SHOPPING_TRIP,
        "Go shopping",
        holder,
    ));
    events.extend(step(PATRICK, OUT_OF_FOOD, PASTA, "Cook pasta", holder));
    events
}

/// The events, in order.
pub fn given(parts: Vec<Vec<FamilyEvent>>) -> Vec<FamilyEvent> {
    parts.into_iter().flatten().collect()
}

/// The family after these events (what every phone shows).
pub fn replay(events: &[FamilyEvent]) -> Family {
    Family::replay(events)
}
