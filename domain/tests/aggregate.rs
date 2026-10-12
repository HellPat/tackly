//! The family aggregate: given what happened, when someone does something,
//! then these events happen (or this error).

#![allow(clippy::expect_used)] // a test fails by panicking

mod support;

use support::*;
use tackly_domain::{DomainEvent as Happened, FamilyCommand as Do, FamilyError};
use uuid::Uuid;

// ---- tasks and lists -------------------------------------------------------------------

#[test]
fn a_new_task_lands_in_other() {
    as_member(PATRICK)
        .given(the_smiths())
        .when(Do::AddTask {
            task_id: MILK,
            list_id: None,
            title: named(" Milk "),
            place_ids: vec![],
        })
        .then_expect_events(vec![by(
            PATRICK,
            MILK,
            Happened::TaskCreated {
                list_id: None,
                title: named("Milk"),
                place_ids: vec![],
                problem_id: None,
            },
        )]);
}

#[test]
fn a_task_can_go_into_a_list_that_exists() {
    as_member(PATRICK)
        .given(given(vec![the_smiths(), vec![shopping_list()]]))
        .when(Do::AddTask {
            task_id: MILK,
            list_id: Some(SHOPPING),
            title: named("Milk"),
            place_ids: vec![],
        })
        .then_expect_events(vec![by(
            PATRICK,
            MILK,
            Happened::TaskCreated {
                list_id: Some(SHOPPING),
                title: named("Milk"),
                place_ids: vec![],
                problem_id: None,
            },
        )]);
}

#[test]
fn a_list_with_open_tasks_cannot_be_deleted() {
    as_member(PATRICK)
        .given(given(vec![
            the_smiths(),
            vec![
                shopping_list(),
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
            ],
        ]))
        .when(Do::DeleteList { list_id: SHOPPING })
        .then_expect_error(FamilyError::ListNotEmpty);
}

#[test]
fn an_empty_list_can_be_deleted() {
    as_member(MONA)
        .given(given(vec![the_smiths(), vec![shopping_list()]]))
        .when(Do::DeleteList { list_id: SHOPPING })
        .then_expect_events(vec![by(MONA, SHOPPING, Happened::ListDeleted)]);
}

// ---- who has a task --------------------------------------------------------------------

#[test]
fn a_free_task_can_be_given_to_someone() {
    as_member(PATRICK)
        .given(given(vec![the_smiths(), vec![task(DISHES, "Dishes")]]))
        .when(Do::AssignTask {
            task_id: DISHES,
            member_id: Some(MONA),
        })
        .then_expect_events(vec![by(
            PATRICK,
            DISHES,
            Happened::TaskAssigned {
                member_id: Some(MONA),
            },
        )]);
}

#[test]
fn who_gave_a_task_can_take_it_back() {
    as_member(PATRICK)
        .given(given(vec![
            the_smiths(),
            vec![
                task(DISHES, "Dishes"),
                by(
                    PATRICK,
                    DISHES,
                    Happened::TaskAssigned {
                        member_id: Some(MONA),
                    },
                ),
            ],
        ]))
        .when(Do::AssignTask {
            task_id: DISHES,
            member_id: None,
        })
        .then_expect_events(vec![by(
            PATRICK,
            DISHES,
            Happened::TaskAssigned { member_id: None },
        )]);
}

#[test]
fn a_task_someone_else_took_cannot_be_given_away() {
    as_member(MARA)
        .given(given(vec![
            the_smiths(),
            vec![
                task(DISHES, "Dishes"),
                by(MONA, DISHES, Happened::TaskStarted),
            ],
        ]))
        .when(Do::AssignTask {
            task_id: DISHES,
            member_id: Some(MARA),
        })
        .then_expect_error(FamilyError::Taken);
}

#[test]
fn a_task_can_only_go_to_a_family_member() {
    as_member(PATRICK)
        .given(given(vec![the_smiths(), vec![task(DISHES, "Dishes")]]))
        .when(Do::AssignTask {
            task_id: DISHES,
            member_id: Some(uuid::Uuid::from_u128(0x99)),
        })
        .then_expect_error(FamilyError::UnknownMember);
}

// ---- working on a task -----------------------------------------------------------------

#[test]
fn starting_a_free_task() {
    as_member(MONA)
        .given(given(vec![the_smiths(), vec![task(DISHES, "Dishes")]]))
        .when(Do::StartTask { task_id: DISHES })
        .then_expect_events(vec![by(MONA, DISHES, Happened::TaskStarted)]);
}

#[test]
fn starting_again_while_working_changes_nothing() {
    as_member(MONA)
        .given(given(vec![
            the_smiths(),
            vec![
                task(DISHES, "Dishes"),
                by(MONA, DISHES, Happened::TaskStarted),
            ],
        ]))
        .when(Do::StartTask { task_id: DISHES })
        .then_expect_events(vec![]);
}

#[test]
fn a_task_someone_else_works_on_cannot_be_started() {
    as_member(PATRICK)
        .given(given(vec![
            the_smiths(),
            vec![
                task(DISHES, "Dishes"),
                by(MONA, DISHES, Happened::TaskStarted),
            ],
        ]))
        .when(Do::StartTask { task_id: DISHES })
        .then_expect_error(FamilyError::Taken);
}

#[test]
fn pausing_needs_running_work() {
    as_member(MONA)
        .given(given(vec![
            the_smiths(),
            vec![
                task(DISHES, "Dishes"),
                by(
                    MONA,
                    DISHES,
                    Happened::TaskAssigned {
                        member_id: Some(MONA),
                    },
                ),
            ],
        ]))
        .when(Do::PauseTask { task_id: DISHES })
        .then_expect_error(FamilyError::NotWorking);
}

// ---- finishing ---------------------------------------------------------------------------

#[test]
fn finishing_a_free_task() {
    as_member(MARA)
        .given(given(vec![the_smiths(), vec![task(MILK, "Milk")]]))
        .when(Do::CompleteTask {
            task_id: MILK,
            location: None,
        })
        .then_expect_events(vec![by(
            MARA,
            MILK,
            Happened::TaskCompleted { location: None },
        )]);
}

#[test]
fn a_task_someone_else_has_cannot_be_finished() {
    as_member(PATRICK)
        .given(given(vec![
            the_smiths(),
            vec![
                task(MILK, "Milk"),
                by(
                    PATRICK,
                    MILK,
                    Happened::TaskAssigned {
                        member_id: Some(MONA),
                    },
                ),
                by(MONA, MILK, Happened::TaskStarted),
            ],
        ]))
        .when(Do::CompleteTask {
            task_id: MILK,
            location: None,
        })
        .then_expect_error(FamilyError::Taken);
}

#[test]
fn only_who_finished_it_can_undo() {
    as_member(MONA)
        .given(given(vec![
            the_smiths(),
            vec![
                task(MILK, "Milk"),
                by(MARA, MILK, Happened::TaskCompleted { location: None }),
            ],
        ]))
        .when(Do::UndoCompletion { task_id: MILK })
        .then_expect_error(FamilyError::NotYours);
}

#[test]
fn undo_right_after_finishing() {
    as_member(MARA)
        .given(given(vec![
            the_smiths(),
            vec![
                task(MILK, "Milk"),
                by(MARA, MILK, Happened::TaskCompleted { location: None }),
            ],
        ]))
        .when(Do::UndoCompletion { task_id: MILK })
        .then_expect_events(vec![by(MARA, MILK, Happened::TaskCompletionUndone)]);
}

// ---- places ------------------------------------------------------------------------------

#[test]
fn a_place_belongs_to_a_group_that_exists() {
    as_member(PATRICK)
        .given(the_smiths())
        .when(Do::CreatePlace {
            place_id: LIDL,
            group_id: GROCERY_STORE,
            name: named("LIDL"),
            location: winnenden(),
        })
        .then_expect_error(FamilyError::UnknownGroup);
}

#[test]
fn a_group_with_places_cannot_be_deleted() {
    as_member(PATRICK)
        .given(given(vec![the_smiths(), lidl_in_the_grocery_store()]))
        .when(Do::DeletePlaceGroup {
            group_id: GROCERY_STORE,
        })
        .then_expect_error(FamilyError::GroupNotEmpty);
}

#[test]
fn a_place_keeps_its_last_location() {
    as_member(PATRICK)
        .given(given(vec![the_smiths(), lidl_in_the_grocery_store()]))
        .when(Do::RemovePlaceLocation {
            place_id: LIDL,
            location_id: winnenden().id,
        })
        .then_expect_error(FamilyError::LastLocation);
}

#[test]
fn a_second_location_can_be_removed_again() {
    as_member(PATRICK)
        .given(given(vec![
            the_smiths(),
            lidl_in_the_grocery_store(),
            vec![by(
                PATRICK,
                LIDL,
                Happened::PlaceLocationAdded {
                    location: backnang(),
                },
            )],
        ]))
        .when(Do::RemovePlaceLocation {
            place_id: LIDL,
            location_id: backnang().id,
        })
        .then_expect_events(vec![by(
            PATRICK,
            LIDL,
            Happened::PlaceLocationRemoved {
                location_id: backnang().id,
            },
        )]);
}

#[test]
fn a_task_can_be_needed_at_a_place() {
    as_member(MONA)
        .given(given(vec![
            the_smiths(),
            lidl_in_the_grocery_store(),
            vec![task(MILK, "Milk")],
        ]))
        .when(Do::AddTaskToPlace {
            task_id: MILK,
            place_id: LIDL,
        })
        .then_expect_events(vec![by(
            MONA,
            MILK,
            Happened::TaskPlaceAdded { place_id: LIDL },
        )]);
}

// ---- the family ------------------------------------------------------------------------------

#[test]
fn a_phone_can_only_create_one_family() {
    as_member(PATRICK)
        .given(the_smiths())
        .when(Do::CreateFamily {
            family_id: SMITHS,
            name: named("Again"),
            owner_name: named("Patrick"),
        })
        .then_expect_error(FamilyError::AlreadyCreated);
}

#[test]
fn members_rename_themselves() {
    as_member(MONA)
        .given(the_smiths())
        .when(Do::Rename { name: named("Mo") })
        .then_expect_events(vec![by(
            MONA,
            SMITHS,
            Happened::MemberRenamed { name: named("Mo") },
        )]);
}

#[test]
fn finishing_records_where_it_happened() {
    as_member(MARA)
        .given(given(vec![the_smiths(), vec![task(MILK, "Milk")]]))
        .when(Do::CompleteTask {
            task_id: MILK,
            location: Some(here()),
        })
        .then_expect_events(vec![by(
            MARA,
            MILK,
            Happened::TaskCompleted {
                location: Some(here()),
            },
        )]);
}

#[test]
fn where_it_was_finished_can_follow_once_the_gps_knows() {
    as_member(MARA)
        .given(given(vec![
            the_smiths(),
            vec![
                task(MILK, "Milk"),
                by(MARA, MILK, Happened::TaskCompleted { location: None }),
            ],
        ]))
        .when(Do::LocateCompletion {
            task_id: MILK,
            location: here(),
        })
        .then_expect_events(vec![by(
            MARA,
            MILK,
            Happened::TaskCompletionLocated { location: here() },
        )]);
}

#[test]
fn only_who_finished_it_says_where() {
    as_member(MONA)
        .given(given(vec![
            the_smiths(),
            vec![
                task(MILK, "Milk"),
                by(MARA, MILK, Happened::TaskCompleted { location: None }),
            ],
        ]))
        .when(Do::LocateCompletion {
            task_id: MILK,
            location: here(),
        })
        .then_expect_error(FamilyError::NotYours);
}

#[test]
fn an_open_task_has_no_place_it_was_finished() {
    as_member(MARA)
        .given(given(vec![the_smiths(), vec![task(MILK, "Milk")]]))
        .when(Do::LocateCompletion {
            task_id: MILK,
            location: here(),
        })
        .then_expect_error(FamilyError::NotDone);
}

#[test]
fn where_it_was_finished_is_said_once() {
    as_member(MARA)
        .given(given(vec![
            the_smiths(),
            vec![
                task(MILK, "Milk"),
                by(
                    MARA,
                    MILK,
                    Happened::TaskCompleted {
                        location: Some(here()),
                    },
                ),
            ],
        ]))
        .when(Do::LocateCompletion {
            task_id: MILK,
            location: here(),
        })
        .then_expect_error(FamilyError::AlreadyLocated);
}

// ---- pictures --------------------------------------------------------------------------------

fn photo(data: &str) -> tackly_domain::Picture {
    tackly_domain::Picture {
        icon: icon("pets"),
        tint: 1,
        photo: Some(
            format!("data:image/jpeg;base64,{data}")
                .parse()
                .expect("a photo"),
        ),
    }
}

#[test]
fn a_member_sets_a_photo_as_their_picture() {
    as_member(MONA)
        .given(the_smiths())
        .when(Do::SetPicture {
            picture: photo("/9j/4AAQSkZJRg=="),
        })
        .then_expect_events(vec![by(
            MONA,
            SMITHS,
            Happened::MemberPictureChanged {
                picture: photo("/9j/4AAQSkZJRg=="),
            },
        )]);
}

// ---- problems -----------------------------------------------------------------------------------

#[test]
fn a_problem_is_created_with_its_solutions() {
    as_member(PATRICK)
        .given(the_smiths())
        .when(Do::CreateProblem {
            problem_id: OUT_OF_FOOD,
            title: named(" Out of food "),
            icon: icon("kitchen"),
            solutions: vec![go_shopping(), cook_pasta()],
        })
        .then_expect_events(out_of_food());
}

#[test]
fn noticing_a_problem_makes_a_step_of_each_solution_for_me() {
    as_member(PATRICK)
        .given(given(vec![the_smiths(), out_of_food()]))
        .when(Do::NoticeProblem {
            problem_id: OUT_OF_FOOD,
            member_id: PATRICK,
            task_ids: vec![SHOPPING_TRIP, PASTA],
        })
        .then_expect_events(out_of_food_noticed_for(PATRICK));
}

#[test]
fn raising_a_problem_gives_its_steps_to_someone_else() {
    as_member(PATRICK)
        .given(given(vec![the_smiths(), out_of_food()]))
        .when(Do::NoticeProblem {
            problem_id: OUT_OF_FOOD,
            member_id: MONA,
            task_ids: vec![SHOPPING_TRIP, PASTA],
        })
        .then_expect_events(out_of_food_noticed_for(MONA));
}

#[test]
fn an_open_problem_is_not_noticed_twice() {
    as_member(MONA)
        .given(given(vec![
            the_smiths(),
            out_of_food(),
            out_of_food_noticed_for(PATRICK),
        ]))
        .when(Do::NoticeProblem {
            problem_id: OUT_OF_FOOD,
            member_id: MONA,
            task_ids: vec![Uuid::from_u128(0x80), Uuid::from_u128(0x81)],
        })
        .then_expect_error(FamilyError::ProblemOpen);
}

#[test]
fn noticing_needs_one_new_task_per_solution() {
    as_member(PATRICK)
        .given(given(vec![the_smiths(), out_of_food()]))
        .when(Do::NoticeProblem {
            problem_id: OUT_OF_FOOD,
            member_id: PATRICK,
            task_ids: vec![SHOPPING_TRIP],
        })
        .then_expect_error(FamilyError::WrongSteps);
}

#[test]
fn assigning_a_problem_hands_over_its_open_steps() {
    as_member(PATRICK)
        .given(given(vec![
            the_smiths(),
            out_of_food(),
            out_of_food_noticed_for(PATRICK),
            vec![by(
                PATRICK,
                PASTA,
                Happened::TaskCompleted { location: None },
            )],
        ]))
        .when(Do::AssignProblem {
            problem_id: OUT_OF_FOOD,
            member_id: MARA,
        })
        .then_expect_events(vec![
            by(
                PATRICK,
                OUT_OF_FOOD,
                Happened::ProblemAssigned { member_id: MARA },
            ),
            by(
                PATRICK,
                SHOPPING_TRIP,
                Happened::TaskAssigned {
                    member_id: Some(MARA),
                },
            ),
        ]);
}

#[test]
fn a_new_solution_of_an_open_problem_is_a_step_right_away() {
    let tidy_up = tackly_domain::Solution {
        id: Uuid::from_u128(0x63),
        title: named("Write a shopping list"),
    };
    let task_id = Uuid::from_u128(0x72);
    let mut expected = vec![by(
        MONA,
        OUT_OF_FOOD,
        Happened::ProblemSolutionAdded {
            solution: tidy_up.clone(),
        },
    )];
    expected.extend(step(
        MONA,
        OUT_OF_FOOD,
        task_id,
        "Write a shopping list",
        PATRICK,
    ));
    as_member(MONA)
        .given(given(vec![
            the_smiths(),
            out_of_food(),
            out_of_food_noticed_for(PATRICK),
        ]))
        .when(Do::AddSolution {
            problem_id: OUT_OF_FOOD,
            solution: tidy_up,
            task_id,
        })
        .then_expect_events(expected);
}

#[test]
fn a_problem_is_resolved_by_hand_and_undo_reopens_it() {
    let open = given(vec![
        the_smiths(),
        out_of_food(),
        out_of_food_noticed_for(PATRICK),
    ]);
    as_member(PATRICK)
        .given(open.clone())
        .when(Do::ResolveProblem {
            problem_id: OUT_OF_FOOD,
        })
        .then_expect_events(vec![by(PATRICK, OUT_OF_FOOD, Happened::ProblemResolved)]);
    let mut resolved = open;
    resolved.push(by(PATRICK, OUT_OF_FOOD, Happened::ProblemResolved));
    as_member(PATRICK)
        .given(resolved)
        .when(Do::ReopenProblem {
            problem_id: OUT_OF_FOOD,
        })
        .then_expect_events(vec![by(PATRICK, OUT_OF_FOOD, Happened::ProblemReopened)]);
}

#[test]
fn a_problem_that_is_not_open_cannot_be_resolved() {
    as_member(PATRICK)
        .given(given(vec![the_smiths(), out_of_food()]))
        .when(Do::ResolveProblem {
            problem_id: OUT_OF_FOOD,
        })
        .then_expect_error(FamilyError::ProblemNotOpen);
}

// ---- plans --------------------------------------------------------------------------------------

fn tomorrow() -> chrono::NaiveDate {
    nine_o_clock().date_naive() + chrono::Duration::days(1)
}

#[test]
fn planning_a_day_takes_free_tasks_and_keeps_my_order() {
    as_member(PATRICK)
        .given(given(vec![
            the_smiths(),
            vec![
                task(MILK, "Milk"),
                task(DISHES, "Dishes"),
                by(
                    PATRICK,
                    DISHES,
                    Happened::TaskAssigned {
                        member_id: Some(PATRICK),
                    },
                ),
            ],
        ]))
        .when(Do::PlanDay {
            day: tomorrow(),
            task_ids: focus(&[DISHES, MILK, DISHES]),
        })
        .then_expect_events(vec![
            by(
                PATRICK,
                MILK,
                Happened::TaskAssigned {
                    member_id: Some(PATRICK),
                },
            ),
            by(
                PATRICK,
                PATRICK,
                Happened::DayPlanned {
                    day: tomorrow(),
                    task_ids: focus(&[DISHES, MILK]),
                },
            ),
        ]);
}

#[test]
fn someone_elses_task_cannot_be_in_my_plan() {
    as_member(PATRICK)
        .given(given(vec![
            the_smiths(),
            vec![
                task(DISHES, "Dishes"),
                by(
                    MONA,
                    DISHES,
                    Happened::TaskAssigned {
                        member_id: Some(MONA),
                    },
                ),
            ],
        ]))
        .when(Do::PlanDay {
            day: tomorrow(),
            task_ids: focus(&[DISHES]),
        })
        .then_expect_error(FamilyError::Taken);
}

// ---- the head, members and agents ------------------------------------------------------------

#[test]
fn the_head_removes_a_member() {
    as_member(PATRICK)
        .given(the_smiths())
        .when(Do::RemoveMember { member_id: MARA })
        .then_expect_events(vec![by(
            PATRICK,
            SMITHS,
            Happened::MemberRemoved { member_id: MARA },
        )]);
}

#[test]
fn only_the_head_removes_and_never_itself() {
    as_member(MONA)
        .given(the_smiths())
        .when(Do::RemoveMember { member_id: MARA })
        .then_expect_error(FamilyError::NotHead);
    as_member(PATRICK)
        .given(the_smiths())
        .when(Do::RemoveMember { member_id: PATRICK })
        .then_expect_error(FamilyError::HeadStays);
    as_member(PATRICK)
        .given(the_smiths())
        .when(Do::RemoveMember {
            member_id: Uuid::from_u128(0x99),
        })
        .then_expect_error(FamilyError::UnknownMember);
}

#[test]
fn the_head_admits_an_agent_for_a_while() {
    let until = nine_o_clock() + chrono::Duration::minutes(30);
    as_member(PATRICK)
        .given(the_smiths())
        .when(Do::AdmitAgent {
            member_id: AGENT,
            name: named("AI agent"),
            until,
        })
        .then_expect_events(vec![by(
            PATRICK,
            SMITHS,
            Happened::AgentAdmitted {
                member_id: AGENT,
                name: named("AI agent"),
                until,
            },
        )]);
    as_member(MONA)
        .given(the_smiths())
        .when(Do::AdmitAgent {
            member_id: AGENT,
            name: named("AI agent"),
            until,
        })
        .then_expect_error(FamilyError::NotHead);
}
