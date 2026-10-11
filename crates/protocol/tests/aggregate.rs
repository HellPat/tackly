//! The family aggregate: given what happened, when someone does something,
//! then these events happen (or this error).

mod support;

use support::*;
use tackly_protocol::{DomainEvent as Happened, FamilyCommand as Do, FamilyError};

// ---- tasks and lists -------------------------------------------------------------------

#[test]
fn a_new_task_lands_in_other() {
    as_member(PATRICK)
        .given(the_smiths())
        .when(Do::AddTask {
            task_id: MILK,
            list_id: None,
            title: " Milk ".into(),
            place_ids: vec![],
        })
        .then_expect_events(vec![by(
            PATRICK,
            MILK,
            Happened::TaskCreated {
                list_id: None,
                title: "Milk".into(),
                place_ids: vec![],
            },
        )]);
}

#[test]
fn a_task_needs_a_title() {
    as_member(PATRICK)
        .given(the_smiths())
        .when(Do::AddTask {
            task_id: MILK,
            list_id: None,
            title: "  ".into(),
            place_ids: vec![],
        })
        .then_expect_error(FamilyError::Empty);
}

#[test]
fn a_task_can_go_into_a_list_that_exists() {
    as_member(PATRICK)
        .given(given(vec![the_smiths(), vec![shopping_list()]]))
        .when(Do::AddTask {
            task_id: MILK,
            list_id: Some(SHOPPING),
            title: "Milk".into(),
            place_ids: vec![],
        })
        .then_expect_events(vec![by(
            PATRICK,
            MILK,
            Happened::TaskCreated {
                list_id: Some(SHOPPING),
                title: "Milk".into(),
                place_ids: vec![],
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
                        title: "Milk".into(),
                        place_ids: vec![],
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
            name: "LIDL".into(),
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
            name: "Again".into(),
            owner_name: "Patrick".into(),
        })
        .then_expect_error(FamilyError::AlreadyCreated);
}

#[test]
fn members_rename_themselves() {
    as_member(MONA)
        .given(the_smiths())
        .when(Do::Rename { name: "Mo".into() })
        .then_expect_events(vec![by(
            MONA,
            SMITHS,
            Happened::MemberRenamed { name: "Mo".into() },
        )]);
}

#[test]
fn finishing_records_where_it_happened() {
    let here = tackly_protocol::GeoPoint {
        latitude: 48.8752,
        longitude: 9.3775,
        accuracy_meters: Some(12.0),
    };
    as_member(MARA)
        .given(given(vec![the_smiths(), vec![task(MILK, "Milk")]]))
        .when(Do::CompleteTask {
            task_id: MILK,
            location: Some(here),
        })
        .then_expect_events(vec![by(
            MARA,
            MILK,
            Happened::TaskCompleted {
                location: Some(here),
            },
        )]);
}

// ---- pictures --------------------------------------------------------------------------------

fn photo(data: &str) -> tackly_protocol::Picture {
    tackly_protocol::Picture {
        icon: "pets".into(),
        tint: 1,
        photo: Some(format!("data:image/jpeg;base64,{data}")),
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

#[test]
fn a_photo_that_is_too_big_is_refused() {
    as_member(MONA)
        .given(the_smiths())
        .when(Do::SetPicture {
            picture: photo(&"A".repeat(tackly_protocol::MAX_PHOTO_BYTES)),
        })
        .then_expect_error(FamilyError::BadPhoto);
}

#[test]
fn only_a_jpeg_is_a_photo() {
    let mut not_a_jpeg = photo("PHN2Zz4=");
    not_a_jpeg.photo = Some("data:image/svg+xml;base64,PHN2Zz4=".into());
    as_member(MONA)
        .given(the_smiths())
        .when(Do::SetPicture {
            picture: not_a_jpeg,
        })
        .then_expect_error(FamilyError::BadPhoto);
}
