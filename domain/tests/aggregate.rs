//! The family aggregate: given what happened, when someone does something,
//! then these events happen (or this error).

#![allow(clippy::expect_used)] // a test fails by panicking

mod support;

use support::*;
use tackly_domain::{
    DomainEvent as Happened, FamilyError, FamilyEvent, MemberCommand, PlaceCommand, ProblemCommand,
    TaskCommand,
};
use uuid::Uuid;

// ---- tasks and lists -------------------------------------------------------------------

#[test]
fn a_new_task_lands_in_other() {
    as_member(PATRICK)
        .given(the_smiths())
        .when(
            TaskCommand::AddTask {
                task_id: MILK,
                list_id: None,
                title: named(" Milk "),
                place_ids: vec![],
            }
            .into(),
        )
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
        .when(
            TaskCommand::AddTask {
                task_id: MILK,
                list_id: Some(SHOPPING),
                title: named("Milk"),
                place_ids: vec![],
            }
            .into(),
        )
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
        .when(TaskCommand::DeleteList { list_id: SHOPPING }.into())
        .then_expect_error(FamilyError::ListNotEmpty);
}

#[test]
fn an_empty_list_can_be_deleted() {
    as_member(MONA)
        .given(given(vec![the_smiths(), vec![shopping_list()]]))
        .when(TaskCommand::DeleteList { list_id: SHOPPING }.into())
        .then_expect_events(vec![by(MONA, SHOPPING, Happened::ListDeleted)]);
}

#[test]
fn a_list_is_created_and_renamed() {
    as_member(PATRICK)
        .given(the_smiths())
        .when(
            TaskCommand::CreateList {
                list_id: SHOPPING,
                name: named("Shopping"),
            }
            .into(),
        )
        .then_expect_events(vec![shopping_list()]);
    as_member(MONA)
        .given(given(vec![the_smiths(), vec![shopping_list()]]))
        .when(
            TaskCommand::RenameList {
                list_id: SHOPPING,
                name: named("Groceries"),
            }
            .into(),
        )
        .then_expect_events(vec![by(
            MONA,
            SHOPPING,
            Happened::ListRenamed {
                name: named("Groceries"),
            },
        )]);
}

#[test]
fn a_list_that_does_not_exist_is_neither_renamed_nor_deleted_nor_used() {
    let unknown = |command: TaskCommand| {
        as_member(PATRICK)
            .given(the_smiths())
            .when(command.into())
            .then_expect_error(FamilyError::UnknownList);
    };
    unknown(TaskCommand::RenameList {
        list_id: SHOPPING,
        name: named("Groceries"),
    });
    unknown(TaskCommand::DeleteList { list_id: SHOPPING });
    unknown(TaskCommand::AddTask {
        task_id: MILK,
        list_id: Some(SHOPPING),
        title: named("Milk"),
        place_ids: vec![],
    });
}

#[test]
fn a_task_is_needed_only_at_places_that_exist() {
    as_member(PATRICK)
        .given(the_smiths())
        .when(
            TaskCommand::AddTask {
                task_id: MILK,
                list_id: None,
                title: named("Milk"),
                place_ids: vec![LIDL],
            }
            .into(),
        )
        .then_expect_error(FamilyError::UnknownPlace);
}

#[test]
fn a_task_is_deleted() {
    as_member(MONA)
        .given(given(vec![the_smiths(), vec![task(MILK, "Milk")]]))
        .when(TaskCommand::DeleteTask { task_id: MILK }.into())
        .then_expect_events(vec![by(MONA, MILK, Happened::TaskDeleted)]);
}

#[test]
fn a_task_that_does_not_exist_cannot_be_changed() {
    let unknown = |command: TaskCommand| {
        as_member(PATRICK)
            .given(given(vec![the_smiths(), lidl_in_the_grocery_store()]))
            .when(command.into())
            .then_expect_error(FamilyError::UnknownTask);
    };
    unknown(TaskCommand::DeleteTask { task_id: MILK });
    unknown(TaskCommand::AddTaskToPlace {
        task_id: MILK,
        place_id: LIDL,
    });
    unknown(TaskCommand::RemoveTaskFromPlace {
        task_id: MILK,
        place_id: LIDL,
    });
    unknown(TaskCommand::AssignTask {
        task_id: MILK,
        member_id: None,
    });
    unknown(TaskCommand::StartTask { task_id: MILK });
    unknown(TaskCommand::LocateCompletion {
        task_id: MILK,
        location: here(),
    });
    unknown(TaskCommand::UndoCompletion { task_id: MILK });
    unknown(TaskCommand::ReopenTask { task_id: MILK });
}

/// Milk, needed at LIDL.
fn milk_at_lidl() -> Vec<FamilyEvent> {
    given(vec![
        the_smiths(),
        lidl_in_the_grocery_store(),
        vec![
            task(MILK, "Milk"),
            by(PATRICK, MILK, Happened::TaskPlaceAdded { place_id: LIDL }),
        ],
    ])
}

#[test]
fn a_task_already_needed_at_a_place_is_not_added_again() {
    as_member(MONA)
        .given(milk_at_lidl())
        .when(
            TaskCommand::AddTaskToPlace {
                task_id: MILK,
                place_id: LIDL,
            }
            .into(),
        )
        .then_expect_events(vec![]);
}

#[test]
fn a_task_is_needed_at_a_place_that_exists() {
    as_member(MONA)
        .given(given(vec![the_smiths(), vec![task(MILK, "Milk")]]))
        .when(
            TaskCommand::AddTaskToPlace {
                task_id: MILK,
                place_id: LIDL,
            }
            .into(),
        )
        .then_expect_error(FamilyError::UnknownPlace);
}

#[test]
fn a_task_is_taken_off_a_place_only_where_it_is() {
    as_member(MONA)
        .given(milk_at_lidl())
        .when(
            TaskCommand::RemoveTaskFromPlace {
                task_id: MILK,
                place_id: LIDL,
            }
            .into(),
        )
        .then_expect_events(vec![by(
            MONA,
            MILK,
            Happened::TaskPlaceRemoved { place_id: LIDL },
        )]);
    as_member(MONA)
        .given(given(vec![the_smiths(), vec![task(MILK, "Milk")]]))
        .when(
            TaskCommand::RemoveTaskFromPlace {
                task_id: MILK,
                place_id: LIDL,
            }
            .into(),
        )
        .then_expect_events(vec![]);
}

// ---- who has a task --------------------------------------------------------------------

#[test]
fn a_free_task_can_be_given_to_someone() {
    as_member(PATRICK)
        .given(given(vec![the_smiths(), vec![task(DISHES, "Dishes")]]))
        .when(
            TaskCommand::AssignTask {
                task_id: DISHES,
                member_id: Some(MONA),
            }
            .into(),
        )
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
        .when(
            TaskCommand::AssignTask {
                task_id: DISHES,
                member_id: None,
            }
            .into(),
        )
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
        .when(
            TaskCommand::AssignTask {
                task_id: DISHES,
                member_id: Some(MARA),
            }
            .into(),
        )
        .then_expect_error(FamilyError::Taken);
}

#[test]
fn a_task_can_only_go_to_a_family_member() {
    as_member(PATRICK)
        .given(given(vec![the_smiths(), vec![task(DISHES, "Dishes")]]))
        .when(
            TaskCommand::AssignTask {
                task_id: DISHES,
                member_id: Some(uuid::Uuid::from_u128(0x99)),
            }
            .into(),
        )
        .then_expect_error(FamilyError::UnknownMember);
}

// ---- working on a task -----------------------------------------------------------------

#[test]
fn starting_a_free_task() {
    as_member(MONA)
        .given(given(vec![the_smiths(), vec![task(DISHES, "Dishes")]]))
        .when(TaskCommand::StartTask { task_id: DISHES }.into())
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
        .when(TaskCommand::StartTask { task_id: DISHES }.into())
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
        .when(TaskCommand::StartTask { task_id: DISHES }.into())
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
        .when(TaskCommand::PauseTask { task_id: DISHES }.into())
        .then_expect_error(FamilyError::NotWorking);
}

#[test]
fn pausing_running_work() {
    as_member(MONA)
        .given(given(vec![
            the_smiths(),
            vec![
                task(DISHES, "Dishes"),
                by(MONA, DISHES, Happened::TaskStarted),
            ],
        ]))
        .when(TaskCommand::PauseTask { task_id: DISHES }.into())
        .then_expect_events(vec![by(MONA, DISHES, Happened::TaskPaused)]);
}

#[test]
fn a_finished_task_cannot_be_worked_on() {
    as_member(MONA)
        .given(given(vec![
            the_smiths(),
            vec![
                task(DISHES, "Dishes"),
                by(MONA, DISHES, Happened::TaskCompleted { location: None }),
            ],
        ]))
        .when(TaskCommand::StartTask { task_id: DISHES }.into())
        .then_expect_error(FamilyError::AlreadyDone);
}

// ---- finishing ---------------------------------------------------------------------------

#[test]
fn finishing_a_free_task() {
    as_member(MARA)
        .given(given(vec![the_smiths(), vec![task(MILK, "Milk")]]))
        .when(
            TaskCommand::CompleteTask {
                task_id: MILK,
                location: None,
            }
            .into(),
        )
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
        .when(
            TaskCommand::CompleteTask {
                task_id: MILK,
                location: None,
            }
            .into(),
        )
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
        .when(TaskCommand::UndoCompletion { task_id: MILK }.into())
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
        .when(TaskCommand::UndoCompletion { task_id: MILK }.into())
        .then_expect_events(vec![by(MARA, MILK, Happened::TaskCompletionUndone)]);
}

#[test]
fn an_open_task_has_nothing_to_undo_or_reopen() {
    let open = |command: TaskCommand| {
        as_member(MARA)
            .given(given(vec![the_smiths(), vec![task(MILK, "Milk")]]))
            .when(command.into())
            .then_expect_error(FamilyError::NotDone);
    };
    open(TaskCommand::UndoCompletion { task_id: MILK });
    open(TaskCommand::ReopenTask { task_id: MILK });
}

#[test]
fn anyone_reopens_a_finished_task() {
    as_member(PATRICK)
        .given(given(vec![
            the_smiths(),
            vec![
                task(MILK, "Milk"),
                by(MARA, MILK, Happened::TaskCompleted { location: None }),
            ],
        ]))
        .when(TaskCommand::ReopenTask { task_id: MILK }.into())
        .then_expect_events(vec![by(PATRICK, MILK, Happened::TaskReopened)]);
}

// ---- places ------------------------------------------------------------------------------

#[test]
fn a_place_belongs_to_a_group_that_exists() {
    as_member(PATRICK)
        .given(the_smiths())
        .when(
            PlaceCommand::CreatePlace {
                place_id: LIDL,
                group_id: GROCERY_STORE,
                name: named("LIDL"),
                location: winnenden(),
            }
            .into(),
        )
        .then_expect_error(FamilyError::UnknownGroup);
}

#[test]
fn a_group_with_places_cannot_be_deleted() {
    as_member(PATRICK)
        .given(given(vec![the_smiths(), lidl_in_the_grocery_store()]))
        .when(
            PlaceCommand::DeletePlaceGroup {
                group_id: GROCERY_STORE,
            }
            .into(),
        )
        .then_expect_error(FamilyError::GroupNotEmpty);
}

#[test]
fn a_place_keeps_its_last_location() {
    as_member(PATRICK)
        .given(given(vec![the_smiths(), lidl_in_the_grocery_store()]))
        .when(
            PlaceCommand::RemovePlaceLocation {
                place_id: LIDL,
                location_id: winnenden().id,
            }
            .into(),
        )
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
        .when(
            PlaceCommand::RemovePlaceLocation {
                place_id: LIDL,
                location_id: backnang().id,
            }
            .into(),
        )
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
        .when(
            TaskCommand::AddTaskToPlace {
                task_id: MILK,
                place_id: LIDL,
            }
            .into(),
        )
        .then_expect_events(vec![by(
            MONA,
            MILK,
            Happened::TaskPlaceAdded { place_id: LIDL },
        )]);
}

/// A second group, for moving places.
const DRUGSTORE: Uuid = Uuid::from_u128(0x31);

#[test]
fn a_place_group_is_created() {
    as_member(PATRICK)
        .given(the_smiths())
        .when(
            PlaceCommand::CreatePlaceGroup {
                group_id: GROCERY_STORE,
                name: named("Grocery Store"),
            }
            .into(),
        )
        .then_expect_events(vec![by(
            PATRICK,
            GROCERY_STORE,
            Happened::PlaceGroupCreated {
                name: named("Grocery Store"),
            },
        )]);
}

#[test]
fn a_group_is_renamed() {
    as_member(MONA)
        .given(given(vec![the_smiths(), lidl_in_the_grocery_store()]))
        .when(
            PlaceCommand::RenamePlaceGroup {
                group_id: GROCERY_STORE,
                name: named("Food"),
            }
            .into(),
        )
        .then_expect_events(vec![by(
            MONA,
            GROCERY_STORE,
            Happened::PlaceGroupRenamed {
                name: named("Food"),
            },
        )]);
}

#[test]
fn a_group_that_does_not_exist_is_neither_renamed_nor_deleted() {
    as_member(PATRICK)
        .given(the_smiths())
        .when(
            PlaceCommand::RenamePlaceGroup {
                group_id: GROCERY_STORE,
                name: named("Food"),
            }
            .into(),
        )
        .then_expect_error(FamilyError::UnknownGroup);
    as_member(PATRICK)
        .given(the_smiths())
        .when(
            PlaceCommand::DeletePlaceGroup {
                group_id: GROCERY_STORE,
            }
            .into(),
        )
        .then_expect_error(FamilyError::UnknownGroup);
}

#[test]
fn an_empty_group_can_be_deleted() {
    as_member(PATRICK)
        .given(given(vec![
            the_smiths(),
            lidl_in_the_grocery_store(),
            vec![by(PATRICK, LIDL, Happened::PlaceDeleted)],
        ]))
        .when(
            PlaceCommand::DeletePlaceGroup {
                group_id: GROCERY_STORE,
            }
            .into(),
        )
        .then_expect_events(vec![by(
            PATRICK,
            GROCERY_STORE,
            Happened::PlaceGroupDeleted,
        )]);
}

#[test]
fn a_place_is_created_in_its_group() {
    as_member(PATRICK)
        .given(given(vec![
            the_smiths(),
            vec![lidl_in_the_grocery_store()[0].clone()],
        ]))
        .when(
            PlaceCommand::CreatePlace {
                place_id: LIDL,
                group_id: GROCERY_STORE,
                name: named("LIDL"),
                location: winnenden(),
            }
            .into(),
        )
        .then_expect_events(vec![lidl_in_the_grocery_store()[1].clone()]);
}

#[test]
fn a_place_is_renamed_moved_given_a_location_and_deleted() {
    let lidl = || given(vec![the_smiths(), lidl_in_the_grocery_store()]);
    as_member(MONA)
        .given(lidl())
        .when(
            PlaceCommand::RenamePlace {
                place_id: LIDL,
                name: named("Lidl"),
            }
            .into(),
        )
        .then_expect_events(vec![by(
            MONA,
            LIDL,
            Happened::PlaceRenamed {
                name: named("Lidl"),
            },
        )]);
    let mut with_drugstore = lidl();
    with_drugstore.push(by(
        PATRICK,
        DRUGSTORE,
        Happened::PlaceGroupCreated {
            name: named("Drugstore"),
        },
    ));
    as_member(MONA)
        .given(with_drugstore)
        .when(
            PlaceCommand::MovePlace {
                place_id: LIDL,
                group_id: DRUGSTORE,
            }
            .into(),
        )
        .then_expect_events(vec![by(
            MONA,
            LIDL,
            Happened::PlaceMoved {
                group_id: DRUGSTORE,
            },
        )]);
    as_member(MONA)
        .given(lidl())
        .when(
            PlaceCommand::AddPlaceLocation {
                place_id: LIDL,
                location: backnang(),
            }
            .into(),
        )
        .then_expect_events(vec![by(
            MONA,
            LIDL,
            Happened::PlaceLocationAdded {
                location: backnang(),
            },
        )]);
    as_member(MONA)
        .given(lidl())
        .when(PlaceCommand::DeletePlace { place_id: LIDL }.into())
        .then_expect_events(vec![by(MONA, LIDL, Happened::PlaceDeleted)]);
}

#[test]
fn a_place_that_does_not_exist_cannot_be_changed() {
    let unknown = |command: PlaceCommand| {
        as_member(PATRICK)
            .given(the_smiths())
            .when(command.into())
            .then_expect_error(FamilyError::UnknownPlace);
    };
    unknown(PlaceCommand::RenamePlace {
        place_id: LIDL,
        name: named("Lidl"),
    });
    unknown(PlaceCommand::MovePlace {
        place_id: LIDL,
        group_id: GROCERY_STORE,
    });
    unknown(PlaceCommand::DeletePlace { place_id: LIDL });
    unknown(PlaceCommand::AddPlaceLocation {
        place_id: LIDL,
        location: backnang(),
    });
    unknown(PlaceCommand::RemovePlaceLocation {
        place_id: LIDL,
        location_id: winnenden().id,
    });
}

#[test]
fn a_place_moves_only_into_a_group_that_exists() {
    as_member(PATRICK)
        .given(given(vec![the_smiths(), lidl_in_the_grocery_store()]))
        .when(
            PlaceCommand::MovePlace {
                place_id: LIDL,
                group_id: DRUGSTORE,
            }
            .into(),
        )
        .then_expect_error(FamilyError::UnknownGroup);
}

// ---- the family ------------------------------------------------------------------------------

#[test]
fn a_phone_creates_a_family_and_others_join_it() {
    as_member(PATRICK)
        .given(vec![])
        .when(
            MemberCommand::CreateFamily {
                family_id: SMITHS,
                name: named("The Smiths"),
                owner_name: named("Patrick"),
            }
            .into(),
        )
        .then_expect_events(vec![the_smiths()[0].clone()]);
    as_member(MONA)
        .given(vec![the_smiths()[0].clone()])
        .when(
            MemberCommand::Join {
                name: named("Mona"),
            }
            .into(),
        )
        .then_expect_events(vec![the_smiths()[1].clone()]);
}

#[test]
fn without_a_family_nobody_joins_or_changes_their_name_or_picture() {
    let alone = |command: MemberCommand| {
        as_member(MONA)
            .given(vec![])
            .when(command.into())
            .then_expect_error(FamilyError::NoFamily);
    };
    alone(MemberCommand::Join {
        name: named("Mona"),
    });
    alone(MemberCommand::Rename { name: named("Mo") });
    alone(MemberCommand::SetPicture {
        picture: tackly_domain::Picture {
            icon: icon("face"),
            tint: 0,
            photo: None,
        },
    });
}

#[test]
fn a_phone_can_only_create_one_family() {
    as_member(PATRICK)
        .given(the_smiths())
        .when(
            MemberCommand::CreateFamily {
                family_id: SMITHS,
                name: named("Again"),
                owner_name: named("Patrick"),
            }
            .into(),
        )
        .then_expect_error(FamilyError::AlreadyCreated);
}

#[test]
fn members_rename_themselves() {
    as_member(MONA)
        .given(the_smiths())
        .when(MemberCommand::Rename { name: named("Mo") }.into())
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
        .when(
            TaskCommand::CompleteTask {
                task_id: MILK,
                location: Some(here()),
            }
            .into(),
        )
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
        .when(
            TaskCommand::LocateCompletion {
                task_id: MILK,
                location: here(),
            }
            .into(),
        )
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
        .when(
            TaskCommand::LocateCompletion {
                task_id: MILK,
                location: here(),
            }
            .into(),
        )
        .then_expect_error(FamilyError::NotYours);
}

#[test]
fn an_open_task_has_no_place_it_was_finished() {
    as_member(MARA)
        .given(given(vec![the_smiths(), vec![task(MILK, "Milk")]]))
        .when(
            TaskCommand::LocateCompletion {
                task_id: MILK,
                location: here(),
            }
            .into(),
        )
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
        .when(
            TaskCommand::LocateCompletion {
                task_id: MILK,
                location: here(),
            }
            .into(),
        )
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
        .when(
            MemberCommand::SetPicture {
                picture: photo("/9j/4AAQSkZJRg=="),
            }
            .into(),
        )
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
        .when(
            ProblemCommand::CreateProblem {
                problem_id: OUT_OF_FOOD,
                title: named(" Out of food "),
                icon: icon("kitchen"),
                solutions: vec![go_shopping(), cook_pasta()],
            }
            .into(),
        )
        .then_expect_events(out_of_food());
}

#[test]
fn noticing_a_problem_makes_a_step_of_each_solution_for_me() {
    as_member(PATRICK)
        .given(given(vec![the_smiths(), out_of_food()]))
        .when(
            ProblemCommand::NoticeProblem {
                problem_id: OUT_OF_FOOD,
                member_id: PATRICK,
                task_ids: vec![SHOPPING_TRIP, PASTA],
            }
            .into(),
        )
        .then_expect_events(out_of_food_noticed_for(PATRICK));
}

#[test]
fn raising_a_problem_gives_its_steps_to_someone_else() {
    as_member(PATRICK)
        .given(given(vec![the_smiths(), out_of_food()]))
        .when(
            ProblemCommand::NoticeProblem {
                problem_id: OUT_OF_FOOD,
                member_id: MONA,
                task_ids: vec![SHOPPING_TRIP, PASTA],
            }
            .into(),
        )
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
        .when(
            ProblemCommand::NoticeProblem {
                problem_id: OUT_OF_FOOD,
                member_id: MONA,
                task_ids: vec![Uuid::from_u128(0x80), Uuid::from_u128(0x81)],
            }
            .into(),
        )
        .then_expect_error(FamilyError::ProblemOpen);
}

#[test]
fn noticing_needs_one_new_task_per_solution() {
    as_member(PATRICK)
        .given(given(vec![the_smiths(), out_of_food()]))
        .when(
            ProblemCommand::NoticeProblem {
                problem_id: OUT_OF_FOOD,
                member_id: PATRICK,
                task_ids: vec![SHOPPING_TRIP],
            }
            .into(),
        )
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
        .when(
            ProblemCommand::AssignProblem {
                problem_id: OUT_OF_FOOD,
                member_id: MARA,
            }
            .into(),
        )
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
        .when(
            ProblemCommand::AddSolution {
                problem_id: OUT_OF_FOOD,
                solution: tidy_up,
                task_id,
            }
            .into(),
        )
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
        .when(
            ProblemCommand::ResolveProblem {
                problem_id: OUT_OF_FOOD,
            }
            .into(),
        )
        .then_expect_events(vec![by(PATRICK, OUT_OF_FOOD, Happened::ProblemResolved)]);
    let mut resolved = open;
    resolved.push(by(PATRICK, OUT_OF_FOOD, Happened::ProblemResolved));
    as_member(PATRICK)
        .given(resolved)
        .when(
            ProblemCommand::ReopenProblem {
                problem_id: OUT_OF_FOOD,
            }
            .into(),
        )
        .then_expect_events(vec![by(PATRICK, OUT_OF_FOOD, Happened::ProblemReopened)]);
}

#[test]
fn a_problem_that_is_not_open_cannot_be_resolved() {
    as_member(PATRICK)
        .given(given(vec![the_smiths(), out_of_food()]))
        .when(
            ProblemCommand::ResolveProblem {
                problem_id: OUT_OF_FOOD,
            }
            .into(),
        )
        .then_expect_error(FamilyError::ProblemNotOpen);
}

#[test]
fn a_problem_is_renamed_and_loses_a_solution() {
    as_member(MONA)
        .given(given(vec![the_smiths(), out_of_food()]))
        .when(
            ProblemCommand::RenameProblem {
                problem_id: OUT_OF_FOOD,
                title: named("Nothing to eat"),
            }
            .into(),
        )
        .then_expect_events(vec![by(
            MONA,
            OUT_OF_FOOD,
            Happened::ProblemRenamed {
                title: named("Nothing to eat"),
            },
        )]);
    as_member(MONA)
        .given(given(vec![the_smiths(), out_of_food()]))
        .when(
            ProblemCommand::RemoveSolution {
                problem_id: OUT_OF_FOOD,
                solution_id: COOK_PASTA,
            }
            .into(),
        )
        .then_expect_events(vec![by(
            MONA,
            OUT_OF_FOOD,
            Happened::ProblemSolutionRemoved {
                solution_id: COOK_PASTA,
            },
        )]);
}

#[test]
fn a_problem_that_does_not_exist_cannot_be_changed() {
    let unknown = |command: ProblemCommand| {
        as_member(PATRICK)
            .given(the_smiths())
            .when(command.into())
            .then_expect_error(FamilyError::UnknownProblem);
    };
    unknown(ProblemCommand::RenameProblem {
        problem_id: OUT_OF_FOOD,
        title: named("Nothing to eat"),
    });
    unknown(ProblemCommand::RemoveSolution {
        problem_id: OUT_OF_FOOD,
        solution_id: COOK_PASTA,
    });
    unknown(ProblemCommand::AssignProblem {
        problem_id: OUT_OF_FOOD,
        member_id: MONA,
    });
    unknown(ProblemCommand::ResolveProblem {
        problem_id: OUT_OF_FOOD,
    });
    unknown(ProblemCommand::ReopenProblem {
        problem_id: OUT_OF_FOOD,
    });
}

#[test]
fn a_problem_is_handed_over_only_while_open() {
    as_member(PATRICK)
        .given(given(vec![the_smiths(), out_of_food()]))
        .when(
            ProblemCommand::AssignProblem {
                problem_id: OUT_OF_FOOD,
                member_id: MONA,
            }
            .into(),
        )
        .then_expect_error(FamilyError::ProblemNotOpen);
}

#[test]
fn only_a_problem_that_was_open_and_is_resolved_reopens() {
    as_member(PATRICK)
        .given(given(vec![
            the_smiths(),
            out_of_food(),
            out_of_food_noticed_for(PATRICK),
        ]))
        .when(
            ProblemCommand::ReopenProblem {
                problem_id: OUT_OF_FOOD,
            }
            .into(),
        )
        .then_expect_error(FamilyError::ProblemOpen);
    as_member(PATRICK)
        .given(given(vec![the_smiths(), out_of_food()]))
        .when(
            ProblemCommand::ReopenProblem {
                problem_id: OUT_OF_FOOD,
            }
            .into(),
        )
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
        .when(
            ProblemCommand::PlanDay {
                day: tomorrow(),
                task_ids: focus(&[DISHES, MILK, DISHES]),
            }
            .into(),
        )
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
        .when(
            ProblemCommand::PlanDay {
                day: tomorrow(),
                task_ids: focus(&[DISHES]),
            }
            .into(),
        )
        .then_expect_error(FamilyError::Taken);
}

// ---- the head, members and agents ------------------------------------------------------------

#[test]
fn the_head_removes_a_member() {
    as_member(PATRICK)
        .given(the_smiths())
        .when(MemberCommand::RemoveMember { member_id: MARA }.into())
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
        .when(MemberCommand::RemoveMember { member_id: MARA }.into())
        .then_expect_error(FamilyError::NotHead);
    as_member(PATRICK)
        .given(the_smiths())
        .when(MemberCommand::RemoveMember { member_id: PATRICK }.into())
        .then_expect_error(FamilyError::HeadStays);
    as_member(PATRICK)
        .given(the_smiths())
        .when(
            MemberCommand::RemoveMember {
                member_id: Uuid::from_u128(0x99),
            }
            .into(),
        )
        .then_expect_error(FamilyError::UnknownMember);
}

#[test]
fn the_head_admits_an_agent_for_a_while() {
    let until = nine_o_clock() + chrono::Duration::minutes(30);
    as_member(PATRICK)
        .given(the_smiths())
        .when(
            MemberCommand::AdmitAgent {
                member_id: AGENT,
                name: named("AI agent"),
                until,
            }
            .into(),
        )
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
        .when(
            MemberCommand::AdmitAgent {
                member_id: AGENT,
                name: named("AI agent"),
                until,
            }
            .into(),
        )
        .then_expect_error(FamilyError::NotHead);
    as_member(PATRICK)
        .given(the_smiths())
        .when(
            MemberCommand::AdmitAgent {
                member_id: PATRICK,
                name: named("AI agent"),
                until,
            }
            .into(),
        )
        .then_expect_error(FamilyError::HeadStays);
}
