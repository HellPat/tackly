//! The projection: given these events (in server order), every phone shows
//! this family. Replay is lenient: events that no longer fit are ignored.

#![allow(clippy::expect_used)] // a test fails by panicking

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
                title: named("Milk"),
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
                Happened::TaskCompletionLocated { location: here() },
            ),
        ],
    ]));

    assert_eq!(
        family.tasks[&MILK]
            .done
            .as_ref()
            .and_then(|done| done.location),
        Some(here())
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
                    title: named("Milk"),
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
fn lists_follow_renames_and_a_list_with_open_tasks_stays() {
    let family = replay(&given(vec![
        the_smiths(),
        vec![
            shopping_list(),
            by(
                MONA,
                SHOPPING,
                Happened::ListRenamed {
                    name: named("Groceries"),
                },
            ),
            by(
                PATRICK,
                MILK,
                Happened::TaskCreated {
                    list_id: Some(SHOPPING),
                    title: named("Milk"),
                    place_ids: vec![],
                    problem_id: None,
                },
            ),
            by(MONA, SHOPPING, Happened::ListDeleted),
            by(
                MONA,
                Uuid::from_u128(0xBAD),
                Happened::ListRenamed { name: named("X") },
            ),
        ],
    ]));

    assert_eq!(family.lists[&SHOPPING].name, "Groceries");
    assert_eq!(family.lists.len(), 1);
}

#[test]
fn a_task_is_created_once_found_by_title_and_deleted() {
    let created_again = by(
        MONA,
        MILK,
        Happened::TaskCreated {
            list_id: None,
            title: named("Other milk"),
            place_ids: vec![],
            problem_id: None,
        },
    );
    let family = replay(&given(vec![
        the_smiths(),
        vec![task(MILK, "Milk"), created_again, task(DISHES, "Dishes")],
    ]));
    assert_eq!(family.tasks[&MILK].title, "Milk");
    assert_eq!(
        family.task_by_title("Dishes").map(|task| task.id),
        Some(DISHES)
    );
    assert!(family.task_by_title("Other milk").is_none());

    let family = replay(&given(vec![
        the_smiths(),
        vec![task(MILK, "Milk"), by(MONA, MILK, Happened::TaskDeleted)],
    ]));
    assert!(family.tasks.is_empty());
}

#[test]
fn a_task_is_needed_at_places_that_exist_and_taken_off_them() {
    let family = replay(&given(vec![
        the_smiths(),
        lidl_in_the_grocery_store(),
        vec![
            task(MILK, "Milk"),
            by(
                MONA,
                MILK,
                Happened::TaskPlaceAdded {
                    place_id: Uuid::from_u128(0xBAD),
                },
            ),
            by(MONA, MILK, Happened::TaskPlaceAdded { place_id: LIDL }),
            by(MONA, MILK, Happened::TaskPlaceAdded { place_id: LIDL }),
        ],
    ]));
    assert_eq!(family.tasks[&MILK].place_ids, vec![LIDL]);

    let family = replay(&given(vec![
        the_smiths(),
        lidl_in_the_grocery_store(),
        vec![
            task(MILK, "Milk"),
            by(MONA, MILK, Happened::TaskPlaceAdded { place_id: LIDL }),
            by(MONA, MILK, Happened::TaskPlaceRemoved { place_id: LIDL }),
            by(
                MONA,
                Uuid::from_u128(0xBAD),
                Happened::TaskPlaceRemoved { place_id: LIDL },
            ),
        ],
    ]));
    assert!(family.tasks[&MILK].place_ids.is_empty());
}

#[test]
fn work_on_a_task_that_is_gone_or_done_changes_nothing() {
    let gone = Uuid::from_u128(0xBAD);
    let family = replay(&given(vec![
        the_smiths(),
        vec![
            task(MILK, "Milk"),
            by(MARA, MILK, Happened::TaskCompleted { location: None }),
            by(
                MONA,
                MILK,
                Happened::TaskAssigned {
                    member_id: Some(MONA),
                },
            ),
            by(MONA, MILK, Happened::TaskStarted),
            by(
                MONA,
                gone,
                Happened::TaskAssigned {
                    member_id: Some(MONA),
                },
            ),
            by(MONA, gone, Happened::TaskStarted),
            by(MONA, gone, Happened::TaskCompleted { location: None }),
            task(DISHES, "Dishes"),
            by(
                MONA,
                DISHES,
                Happened::TaskAssigned {
                    member_id: Some(gone),
                },
            ),
        ],
    ]));

    let milk = &family.tasks[&MILK];
    assert_eq!(milk.assignee, None);
    assert!(milk.sessions.is_empty());
    assert_eq!(family.tasks[&DISHES].assignee, None);
    assert_eq!(family.tasks.len(), 2);
}

#[test]
fn giving_myself_the_task_i_work_on_keeps_the_work_going() {
    let family = replay(&given(vec![
        the_smiths(),
        vec![
            task(DISHES, "Dishes"),
            by(MONA, DISHES, Happened::TaskStarted),
            by(
                MONA,
                DISHES,
                Happened::TaskAssigned {
                    member_id: Some(MONA),
                },
            ),
        ],
    ]));
    assert_eq!(family.tasks[&DISHES].progress(), Some(Progress::Working));
}

#[test]
fn an_empty_list_is_deleted() {
    let family = replay(&given(vec![
        the_smiths(),
        vec![shopping_list(), by(MONA, SHOPPING, Happened::ListDeleted)],
    ]));
    assert!(family.lists.is_empty());
}

#[test]
fn someone_working_shows_as_working() {
    let family = replay(&given(vec![
        the_smiths(),
        vec![
            task(DISHES, "Dishes"),
            by(MONA, DISHES, Happened::TaskStarted),
        ],
    ]));
    assert_eq!(family.tasks[&DISHES].progress(), Some(Progress::Working));
}

/// A second group, for moving places.
const DRUGSTORE: Uuid = Uuid::from_u128(0x31);

#[test]
fn groups_and_places_follow_their_renames_and_moves() {
    let family = replay(&given(vec![
        the_smiths(),
        lidl_in_the_grocery_store(),
        vec![
            by(
                MONA,
                GROCERY_STORE,
                Happened::PlaceGroupRenamed {
                    name: named("Food"),
                },
            ),
            by(
                PATRICK,
                DRUGSTORE,
                Happened::PlaceGroupCreated {
                    name: named("Drugstore"),
                },
            ),
            by(
                MONA,
                LIDL,
                Happened::PlaceRenamed {
                    name: named("Lidl"),
                },
            ),
            by(
                MONA,
                LIDL,
                Happened::PlaceMoved {
                    group_id: DRUGSTORE,
                },
            ),
        ],
    ]));

    assert_eq!(family.place_groups[&GROCERY_STORE].name, "Food");
    assert_eq!(family.places[&LIDL].name, "Lidl");
    assert_eq!(family.places[&LIDL].group_id, DRUGSTORE);
    assert_eq!(family.places_in(DRUGSTORE).count(), 1);
    assert_eq!(family.places_in(GROCERY_STORE).count(), 0);
}

#[test]
fn what_points_at_a_group_or_place_that_is_gone_changes_nothing() {
    let unknown = Uuid::from_u128(0xBAD);
    let family = replay(&given(vec![
        the_smiths(),
        lidl_in_the_grocery_store(),
        vec![
            by(
                MONA,
                unknown,
                Happened::PlaceGroupRenamed { name: named("X") },
            ),
            by(MONA, unknown, Happened::PlaceRenamed { name: named("X") }),
            by(
                MONA,
                unknown,
                Happened::PlaceMoved {
                    group_id: GROCERY_STORE,
                },
            ),
            by(MONA, LIDL, Happened::PlaceMoved { group_id: unknown }),
            by(MONA, unknown, Happened::PlaceDeleted),
            by(
                MONA,
                unknown,
                Happened::PlaceLocationAdded {
                    location: backnang(),
                },
            ),
            by(
                MONA,
                unknown,
                Happened::PlaceLocationRemoved {
                    location_id: winnenden().id,
                },
            ),
            by(
                MONA,
                unknown,
                Happened::PlaceCreated {
                    group_id: unknown,
                    name: named("Nowhere"),
                    location: winnenden(),
                },
            ),
        ],
    ]));

    assert_eq!(family.place_groups.len(), 1);
    assert_eq!(family.place_groups[&GROCERY_STORE].name, "Grocery Store");
    assert_eq!(family.places.len(), 1);
    assert_eq!(family.places[&LIDL].name, "LIDL");
    assert_eq!(family.places[&LIDL].group_id, GROCERY_STORE);
}

#[test]
fn an_empty_group_is_deleted() {
    let family = replay(&given(vec![
        the_smiths(),
        lidl_in_the_grocery_store(),
        vec![
            by(MONA, LIDL, Happened::PlaceDeleted),
            by(MONA, GROCERY_STORE, Happened::PlaceGroupDeleted),
        ],
    ]));

    assert!(family.place_groups.is_empty());
}

#[test]
fn a_location_is_added_once_and_the_last_one_stays() {
    let added = || {
        by(
            MONA,
            LIDL,
            Happened::PlaceLocationAdded {
                location: backnang(),
            },
        )
    };
    let family = replay(&given(vec![
        the_smiths(),
        lidl_in_the_grocery_store(),
        vec![added(), added()],
    ]));
    assert_eq!(
        family.places[&LIDL].locations,
        vec![winnenden(), backnang()]
    );

    let family = replay(&given(vec![
        the_smiths(),
        lidl_in_the_grocery_store(),
        vec![
            added(),
            by(
                MONA,
                LIDL,
                Happened::PlaceLocationRemoved {
                    location_id: winnenden().id,
                },
            ),
            by(
                MONA,
                LIDL,
                Happened::PlaceLocationRemoved {
                    location_id: backnang().id,
                },
            ),
        ],
    ]));
    assert_eq!(family.places[&LIDL].locations, vec![backnang()]);
}

#[test]
fn open_tasks_at_a_place_leave_out_the_rest() {
    let at_lidl = |id, title: &str| {
        by(
            PATRICK,
            id,
            Happened::TaskCreated {
                list_id: None,
                title: named(title),
                place_ids: vec![LIDL],
                problem_id: None,
            },
        )
    };
    let family = replay(&given(vec![
        the_smiths(),
        lidl_in_the_grocery_store(),
        vec![
            at_lidl(MILK, "Milk"),
            at_lidl(DISHES, "Soap"),
            task(Uuid::from_u128(0x52), "Elsewhere"),
            by(MONA, DISHES, Happened::TaskCompleted { location: None }),
        ],
    ]));

    let open: Vec<Uuid> = family.open_tasks_at(LIDL).map(|task| task.id).collect();
    assert_eq!(open, vec![MILK]);
}

#[test]
fn members_names_and_pictures_follow_their_changes() {
    let family = replay(&given(vec![
        the_smiths(),
        vec![
            by(MONA, SMITHS, Happened::MemberRenamed { name: named("Mo") }),
            by(
                MONA,
                SMITHS,
                Happened::MemberPictureChanged {
                    picture: tackly_domain::Picture {
                        icon: icon("pets"),
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
                    name: named("Fake"),
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
                    name: named("AI agent"),
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
    assert!(!family.is_agent(MONA), "a person is no agent");
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

#[test]
fn a_second_family_created_on_the_same_phone_is_ignored() {
    let family = replay(&given(vec![
        the_smiths(),
        vec![by(
            MONA,
            Uuid::from_u128(0x11),
            Happened::FamilyCreated {
                name: named("The Joneses"),
                owner_name: named("Mona"),
            },
        )],
    ]));
    assert_eq!(family.family_id, Some(SMITHS));
    assert_eq!(family.name.as_deref(), Some("The Smiths"));
    assert_eq!(family.head(), Some(PATRICK));
}

#[test]
fn a_family_is_written_and_read_back_whole() {
    let family = replay(&given(vec![
        the_smiths(),
        vec![shopping_list()],
        lidl_in_the_grocery_store(),
        vec![task(MILK, "Milk"), by(MONA, MILK, Happened::TaskStarted)],
        out_of_food(),
        out_of_food_noticed_for(MONA),
    ]));
    let json = serde_json::to_string(&family).expect("written");
    let back: tackly_domain::Family = serde_json::from_str(&json).expect("read back");
    assert_eq!(back, family);
}

#[test]
fn an_event_from_a_newer_app_is_skipped() {
    let family = replay(&given(vec![
        the_smiths(),
        vec![by(MONA, SMITHS, Happened::Unknown)],
    ]));
    assert_eq!(family, replay(&the_smiths()));
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
fn problems_follow_renames_and_lose_removed_solutions() {
    let gone = Uuid::from_u128(0xBAD);
    let family = replay(&given(vec![
        the_smiths(),
        out_of_food(),
        vec![
            by(
                MONA,
                OUT_OF_FOOD,
                Happened::ProblemRenamed {
                    title: named("Nothing to eat"),
                },
            ),
            by(
                MONA,
                OUT_OF_FOOD,
                Happened::ProblemSolutionRemoved {
                    solution_id: COOK_PASTA,
                },
            ),
            by(MONA, gone, Happened::ProblemRenamed { title: named("X") }),
            by(
                MONA,
                gone,
                Happened::ProblemSolutionRemoved {
                    solution_id: GO_SHOPPING,
                },
            ),
        ],
    ]));
    let problem = &family.problems[&OUT_OF_FOOD];
    assert_eq!(problem.title, "Nothing to eat");
    assert_eq!(problem.solutions, vec![go_shopping()]);
    assert_eq!(family.problems.len(), 1);
}

#[test]
fn a_handed_over_problem_is_open_and_held_by_the_new_member() {
    let family = replay(&given(vec![
        the_smiths(),
        out_of_food(),
        out_of_food_noticed_for(MONA),
        vec![by(
            MONA,
            OUT_OF_FOOD,
            Happened::ProblemAssigned { member_id: MARA },
        )],
    ]));
    let open: Vec<Uuid> = family.open_problems().map(|problem| problem.id).collect();
    assert_eq!(open, vec![OUT_OF_FOOD]);
    assert_eq!(family.problems[&OUT_OF_FOOD].holder(), Some(MARA));
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
                    task_ids: focus(&[DISHES, MILK]),
                },
            ),
        ],
    ]));
    let planned: Vec<&str> = family
        .focus(PATRICK, day)
        .iter()
        .map(|t| t.title.as_str())
        .collect();
    assert_eq!(planned, vec!["Dishes", "Milk"]);

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
                    task_ids: focus(&[DISHES, MILK]),
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
