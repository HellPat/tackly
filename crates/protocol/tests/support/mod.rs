//! The vocabulary the aggregate and projection tests are written in: a family
//! of three, a few lists, places and tasks with fixed IDs, and short ways to
//! say "Patrick did this to that".

#![allow(dead_code)] // each test file uses part of it

use chrono::{DateTime, TimeZone, Utc};
use cqrs_es::test::TestFramework;
use tackly_protocol::{CommandContext, DomainEvent, Family, FamilyEvent, PlaceLocation};
use uuid::Uuid;

pub const PATRICK: Uuid = Uuid::from_u128(0x1);
pub const MONA: Uuid = Uuid::from_u128(0x2);
pub const MARA: Uuid = Uuid::from_u128(0x3);

pub const SMITHS: Uuid = Uuid::from_u128(0x10);
pub const SHOPPING: Uuid = Uuid::from_u128(0x20);
pub const GROCERY_STORE: Uuid = Uuid::from_u128(0x30);
pub const LIDL: Uuid = Uuid::from_u128(0x40);
pub const MILK: Uuid = Uuid::from_u128(0x50);
pub const DISHES: Uuid = Uuid::from_u128(0x51);

/// Every event in these tests has this ID and happens at 9:00 (plus minutes).
pub const EVENT: Uuid = Uuid::from_u128(0xE);

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
                name: "The Smiths".into(),
                owner_name: "Patrick".into(),
            },
        ),
        by(
            MONA,
            SMITHS,
            DomainEvent::MemberJoined {
                name: "Mona".into(),
            },
        ),
        by(
            MARA,
            SMITHS,
            DomainEvent::MemberJoined {
                name: "Mara".into(),
            },
        ),
    ]
}

pub fn shopping_list() -> FamilyEvent {
    by(
        PATRICK,
        SHOPPING,
        DomainEvent::ListCreated {
            name: "Shopping".into(),
        },
    )
}

pub fn lidl_in_the_grocery_store() -> Vec<FamilyEvent> {
    vec![
        by(
            PATRICK,
            GROCERY_STORE,
            DomainEvent::PlaceGroupCreated {
                name: "Grocery Store".into(),
            },
        ),
        by(
            PATRICK,
            LIDL,
            DomainEvent::PlaceCreated {
                group_id: GROCERY_STORE,
                name: "LIDL".into(),
                location: winnenden(),
            },
        ),
    ]
}

pub fn winnenden() -> PlaceLocation {
    PlaceLocation {
        id: Uuid::from_u128(0x41),
        name: "LIDL Winnenden".into(),
        address: None,
        point: None,
    }
}

pub fn backnang() -> PlaceLocation {
    PlaceLocation {
        id: Uuid::from_u128(0x42),
        name: "LIDL Backnang".into(),
        address: None,
        point: None,
    }
}

/// A task created by Patrick, in "Other".
pub fn task(id: Uuid, title: &str) -> FamilyEvent {
    by(
        PATRICK,
        id,
        DomainEvent::TaskCreated {
            list_id: None,
            title: title.into(),
            place_ids: vec![],
        },
    )
}

/// The events, in order.
pub fn given(parts: Vec<Vec<FamilyEvent>>) -> Vec<FamilyEvent> {
    parts.into_iter().flatten().collect()
}

/// The family after these events (what every phone shows).
pub fn replay(events: &[FamilyEvent]) -> Family {
    Family::replay(events)
}
