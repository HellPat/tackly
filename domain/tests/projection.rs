//! The projection: given these events (in server order), every phone shows
//! this family. Replay is lenient: events that no longer fit are ignored.

mod support;

use chrono::Duration;
use support::*;
use tackly_domain::{DomainEvent as Happened, FamilyEvent, Progress};
use uuid::Uuid;

/// The same event, `minutes` after 9:00.
fn later(mut event: FamilyEvent, minutes: i64) -> FamilyEvent {
    event.occurred_at = nine_o_clock() + Duration::minutes(minutes);
    event
}

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
                problem_id: None,
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
                    problem_id: None,
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
                    picture: tackly_domain::Picture {
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

#[test]
fn removed_and_admitted_count_only_from_the_head() {
    let until = nine_o_clock() + chrono::Duration::minutes(30);
    let family = replay(&given(vec![
        the_smiths(),
        vec![
            // Mona has no say.
            by(MONA, SMITHS, Happened::MemberRemoved { member_id: MARA }),
            by(
                MONA,
                SMITHS,
                Happened::AgentAdmitted {
                    member_id: AGENT,
                    name: "Fake".into(),
                    until,
                },
            ),
        ],
    ]));
    assert!(family.members.contains_key(&MARA));
    assert!(!family.members.contains_key(&AGENT));

    let family = replay(&given(vec![
        the_smiths(),
        vec![
            by(PATRICK, SMITHS, Happened::MemberRemoved { member_id: MARA }),
            by(
                PATRICK,
                SMITHS,
                Happened::AgentAdmitted {
                    member_id: AGENT,
                    name: "AI agent".into(),
                    until,
                },
            ),
            by(
                PATRICK,
                SMITHS,
                Happened::MemberRemoved { member_id: PATRICK },
            ),
        ],
    ]));
    assert!(!family.members.contains_key(&MARA));
    assert!(family.is_agent(AGENT));
    assert_eq!(family.members[&AGENT].agent_until, Some(until));
    assert_eq!(family.head(), Some(PATRICK), "the head stays");
    assert_eq!(
        family
            .people()
            .map(|member| member.device_id)
            .collect::<Vec<_>>(),
        vec![PATRICK, MONA]
    );
    assert_eq!(family.agents().count(), 1);
}

// ---- problems and plans ---------------------------------------------------------------------------

#[test]
fn an_open_problem_shows_its_steps_and_who_has_it() {
    let family = replay(&given(vec![
        the_smiths(),
        out_of_food(),
        out_of_food_noticed_for(MONA),
    ]));
    let problem = &family.problems[&OUT_OF_FOOD];
    assert_eq!(problem.holder(), Some(MONA));
    let steps: Vec<&str> = family
        .steps(OUT_OF_FOOD)
        .map(|task| task.title.as_str())
        .collect();
    assert_eq!(steps, vec!["Go shopping", "Cook pasta"]);
    assert_eq!(
        family.open_tasks_in(None).count(),
        0,
        "steps are not in Other"
    );
}

#[test]
fn a_resolved_problem_hides_its_steps_and_reopening_brings_them_back() {
    let mut events = given(vec![
        the_smiths(),
        out_of_food(),
        out_of_food_noticed_for(PATRICK),
    ]);
    events.push(by_at(PATRICK, OUT_OF_FOOD, 5, Happened::ProblemResolved));
    let resolved = replay(&events);
    assert_eq!(resolved.open_problems().count(), 0);
    assert_eq!(resolved.steps(OUT_OF_FOOD).count(), 0);

    events.push(by_at(PATRICK, OUT_OF_FOOD, 6, Happened::ProblemReopened));
    assert_eq!(replay(&events).steps(OUT_OF_FOOD).count(), 2);
}

#[test]
fn noticed_again_it_has_only_the_new_steps() {
    let mut events = given(vec![
        the_smiths(),
        out_of_food(),
        out_of_food_noticed_for(PATRICK),
    ]);
    events.push(by_at(PATRICK, OUT_OF_FOOD, 5, Happened::ProblemResolved));
    events.push(by_at(
        PATRICK,
        OUT_OF_FOOD,
        60,
        Happened::ProblemNoticed { member_id: MONA },
    ));
    for event in step(
        PATRICK,
        OUT_OF_FOOD,
        Uuid::from_u128(0x90),
        "Go shopping",
        MONA,
    ) {
        events.push(later(event, 60));
    }
    let family = replay(&events);
    let steps: Vec<Uuid> = family.steps(OUT_OF_FOOD).map(|task| task.id).collect();
    assert_eq!(steps, vec![Uuid::from_u128(0x90)]);
    assert_eq!(family.problems[&OUT_OF_FOOD].holder(), Some(MONA));
}

#[test]
fn a_plan_is_my_focus_in_my_order_without_what_is_done() {
    let day = nine_o_clock().date_naive();
    let family = replay(&given(vec![
        the_smiths(),
        vec![
            task(MILK, "Milk"),
            task(DISHES, "Dishes"),
            by(
                PATRICK,
                MILK,
                Happened::TaskAssigned {
                    member_id: Some(PATRICK),
                },
            ),
            by(
                PATRICK,
                DISHES,
                Happened::TaskAssigned {
                    member_id: Some(PATRICK),
                },
            ),
            by(
                PATRICK,
                PATRICK,
                Happened::DayPlanned {
                    day,
                    task_ids: vec![DISHES, MILK],
                },
            ),
        ],
    ]));
    let focus: Vec<&str> = family
        .focus(PATRICK, day)
        .iter()
        .map(|t| t.title.as_str())
        .collect();
    assert_eq!(focus, vec!["Dishes", "Milk"]);

    let done = replay(&given(vec![
        the_smiths(),
        vec![
            task(MILK, "Milk"),
            task(DISHES, "Dishes"),
            by(
                PATRICK,
                MILK,
                Happened::TaskAssigned {
                    member_id: Some(PATRICK),
                },
            ),
            by(
                PATRICK,
                DISHES,
                Happened::TaskAssigned {
                    member_id: Some(PATRICK),
                },
            ),
            by(
                PATRICK,
                PATRICK,
                Happened::DayPlanned {
                    day,
                    task_ids: vec![DISHES, MILK],
                },
            ),
            by(PATRICK, DISHES, Happened::TaskCompleted { location: None }),
        ],
    ]));
    let focus: Vec<&str> = done
        .focus(PATRICK, day)
        .iter()
        .map(|t| t.title.as_str())
        .collect();
    assert_eq!(focus, vec!["Milk"]);
    assert!(done.plan(MONA, day).is_none(), "Mona has not planned");
}
