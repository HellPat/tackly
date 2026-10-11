//! The projection: given these events (in server order), every phone shows
//! this family. Replay is lenient: events that no longer fit are ignored.

mod support;

use chrono::Duration;
use support::*;
use tackly_protocol::{DomainEvent as Happened, Progress};

#[test]
fn tasks_without_a_list_are_in_other() {
    let family = replay(&given(vec![the_smiths(), vec![task(MILK, "Milk")]]));

    assert_eq!(family.open_tasks_in(None).count(), 1);
}

#[test]
fn a_task_for_a_list_that_is_gone_ends_up_in_other() {
    let family = replay(&given(vec![
        the_smiths(),
        vec![by(
            PATRICK,
            MILK,
            Happened::TaskCreated {
                list_id: Some(SHOPPING),
                title: "Milk".into(),
                place_ids: vec![],
            },
        )],
    ]));

    assert_eq!(family.tasks[&MILK].list_id, None);
}

#[test]
fn starting_takes_the_task_and_pausing_keeps_it() {
    let family = replay(&given(vec![
        the_smiths(),
        vec![
            task(DISHES, "Dishes"),
            by_at(MONA, DISHES, 0, Happened::TaskStarted),
            by_at(MONA, DISHES, 10, Happened::TaskPaused),
        ],
    ]));
    let dishes = &family.tasks[&DISHES];

    assert_eq!(dishes.assignee, Some(MONA));
    assert_eq!(dishes.progress(), Some(Progress::Paused));
}

#[test]
fn time_worked_adds_up_the_sessions_and_today_starts_at_midnight() {
    let family = replay(&given(vec![
        the_smiths(),
        vec![
            task(DISHES, "Dishes"),
            by_at(MONA, DISHES, -24 * 60, Happened::TaskStarted), // yesterday, 9:00–9:30
            by_at(MONA, DISHES, -24 * 60 + 30, Happened::TaskPaused),
            by_at(MONA, DISHES, 0, Happened::TaskStarted), // today, 9:00–9:15
            by_at(MONA, DISHES, 15, Happened::TaskPaused),
        ],
    ]));
    let midnight = nine_o_clock() - Duration::hours(9);

    let (total, today) =
        family.tasks[&DISHES].worked(nine_o_clock() + Duration::hours(1), midnight);

    assert_eq!(total, Duration::minutes(45));
    assert_eq!(today, Duration::minutes(15));
}

#[test]
fn undo_keeps_the_time_and_reopening_starts_over() {
    let worked = vec![
        task(DISHES, "Dishes"),
        by_at(MONA, DISHES, 0, Happened::TaskStarted),
        by_at(MONA, DISHES, 20, Happened::TaskCompleted { location: None }),
    ];

    let undone = replay(&given(vec![
        the_smiths(),
        worked.clone(),
        vec![by_at(MONA, DISHES, 21, Happened::TaskCompletionUndone)],
    ]));
    assert!(!undone.tasks[&DISHES].is_done());
    assert_eq!(undone.tasks[&DISHES].sessions.len(), 1);

    let reopened = replay(&given(vec![
        the_smiths(),
        worked,
        vec![by_at(PATRICK, DISHES, 90, Happened::TaskReopened)],
    ]));
    assert!(reopened.tasks[&DISHES].sessions.is_empty());
    assert_eq!(reopened.tasks[&DISHES].assignee, None);
}

#[test]
fn finished_twice_is_just_finished_by_the_first() {
    let family = replay(&given(vec![
        the_smiths(),
        vec![
            task(MILK, "Milk"),
            by_at(MONA, MILK, 1, Happened::TaskCompleted { location: None }),
            by_at(MARA, MILK, 2, Happened::TaskCompleted { location: None }),
        ],
    ]));

    assert_eq!(
        family.tasks[&MILK].done.as_ref().map(|done| done.by),
        Some(MONA)
    );
}

#[test]
fn where_it_was_finished_arrives_after_the_tick() {
    let family = replay(&given(vec![
        the_smiths(),
        vec![
            task(MILK, "Milk"),
            by_at(MARA, MILK, 1, Happened::TaskCompleted { location: None }),
            by_at(
                MARA,
                MILK,
                1,
                Happened::TaskCompletionLocated { location: HERE },
            ),
        ],
    ]));

    assert_eq!(
        family.tasks[&MILK]
            .done
            .as_ref()
            .and_then(|done| done.location),
        Some(HERE)
    );
}

#[test]
fn given_away_shows_who_gave_it() {
    let family = replay(&given(vec![
        the_smiths(),
        vec![
            task(DISHES, "Dishes"),
            by(
                PATRICK,
                DISHES,
                Happened::TaskAssigned {
                    member_id: Some(MARA),
                },
            ),
        ],
    ]));
    let dishes = &family.tasks[&DISHES];

    assert_eq!(
        (dishes.assignee, dishes.assigned_by),
        (Some(MARA), Some(PATRICK))
    );
    assert_eq!(dishes.progress(), Some(Progress::Picked));
}

#[test]
fn a_deleted_place_disappears_from_its_tasks() {
    let family = replay(&given(vec![
        the_smiths(),
        lidl_in_the_grocery_store(),
        vec![
            by(
                PATRICK,
                MILK,
                Happened::TaskCreated {
                    list_id: None,
                    title: "Milk".into(),
                    place_ids: vec![LIDL],
                },
            ),
            by(PATRICK, LIDL, Happened::PlaceDeleted),
        ],
    ]));

    assert!(family.tasks[&MILK].place_ids.is_empty());
    assert!(family.places.is_empty());
}

#[test]
fn a_group_that_still_has_places_is_not_deleted() {
    let family = replay(&given(vec![
        the_smiths(),
        lidl_in_the_grocery_store(),
        vec![by(MONA, GROCERY_STORE, Happened::PlaceGroupDeleted)],
    ]));

    assert!(family.place_groups.contains_key(&GROCERY_STORE));
}

#[test]
fn members_names_and_pictures_follow_their_changes() {
    let family = replay(&given(vec![
        the_smiths(),
        vec![
            by(MONA, SMITHS, Happened::MemberRenamed { name: "Mo".into() }),
            by(
                MONA,
                SMITHS,
                Happened::MemberPictureChanged {
                    picture: tackly_protocol::Picture {
                        icon: "pets".into(),
                        tint: 2,
                        photo: None,
                    },
                },
            ),
        ],
    ]));

    assert_eq!(family.member_name(MONA), "Mo");
    assert_eq!(
        family.members[&MONA]
            .picture
            .as_ref()
            .map(|picture| picture.icon.as_str()),
        Some("pets")
    );
    assert!(family.members[&PATRICK].owner);
}
