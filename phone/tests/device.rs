//! One phone on its own: everything a person does goes into the family's log
//! and shows in the family at once, with no relay at all (the family was
//! created offline and registers later).

use anyhow::{Context, Result, ensure};
use tackly_domain::{GeoPoint, PlaceLocation, Progress};
use tackly_phone::{Device, infrastructure::server_url::ServerUrl};
use uuid::Uuid;

/// A phone whose family was created while the relay was out of reach.
async fn alone(dir: &tempfile::TempDir) -> Result<Device> {
    let nowhere = std::net::TcpListener::bind("127.0.0.1:0")?.local_addr()?;
    let mut phone = Device::open(dir.path().join("phone"))?;
    phone
        .create_family(&ServerUrl::http(nowhere), "The Smiths", "Patrick")
        .await?;
    ensure!(
        !phone.membership().context("in a family")?.registered,
        "registers later"
    );
    Ok(phone)
}

/// A relay that answers every request with `status` and an error.
async fn answering(status: &'static str) -> Result<ServerUrl> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let mut request = [0; 4096];
            let _ = stream.read(&mut request).await;
            let body = r#"{"error":"no"}"#;
            let answer = format!(
                "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(answer.as_bytes()).await;
        }
    });
    Ok(ServerUrl::http(addr))
}

#[tokio::test]
async fn a_relay_that_refuses_a_new_family_stops_it_and_one_that_fails_does_not() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let mut refused = Device::open(dir.path().join("refused"))?;
    ensure!(
        refused
            .create_family(&answering("409 Conflict").await?, "The Smiths", "Patrick")
            .await
            .is_err(),
        "refused"
    );
    ensure!(refused.membership().is_none(), "no family");

    let mut failed = Device::open(dir.path().join("failed"))?;
    failed
        .create_family(
            &answering("503 Service Unavailable").await?,
            "The Smiths",
            "Patrick",
        )
        .await?;
    ensure!(
        !failed.membership().context("a family")?.registered,
        "registers later"
    );
    Ok(())
}

fn location(name: &str) -> Result<PlaceLocation> {
    Ok(PlaceLocation::named(name.parse()?))
}

#[tokio::test]
async fn places_are_grouped_moved_located_and_given_tasks() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let mut phone = alone(&dir).await?;
    let food = phone.create_place_group("Food").await?;
    phone.rename_place_group(food, "Grocery Store").await?;
    let winnenden = location("LIDL Winnenden")?;
    let lidl = phone.create_place(food, "Lidl", winnenden.clone()).await?;
    phone.rename_place(lidl, "LIDL").await?;
    let shops = phone.create_place_group("Shops").await?;
    phone.move_place(lidl, shops).await?;
    phone.delete_place_group(food).await?;
    let backnang = location("LIDL Backnang")?;
    phone.add_place_location(lidl, backnang.clone()).await?;
    phone.remove_place_location(lidl, winnenden.id).await?;
    let milk = phone.add_task("Milk", None, &[]).await?;
    phone.add_task_to_place(milk, lidl).await?;

    let family = phone.state()?;
    ensure!(family.place_groups.len() == 1 && family.place_groups[&shops].name == "Shops");
    let place = &family.places[&lidl];
    ensure!(place.name == "LIDL" && place.group_id == shops);
    ensure!(place.locations == vec![backnang]);
    ensure!(family.tasks[&milk].place_ids == vec![lidl]);

    phone.remove_task_from_place(milk, lidl).await?;
    phone.delete_place(lidl).await?;
    let family = phone.state()?;
    ensure!(family.places.is_empty() && family.tasks[&milk].place_ids.is_empty());
    ensure!(
        phone.rename_place(lidl, "Gone").await.is_err(),
        "it is gone"
    );
    ensure!(
        phone.create_place_group(" ").await.is_err(),
        "names are checked"
    );
    Ok(())
}

#[tokio::test]
async fn tasks_are_given_worked_on_finished_undone_and_deleted() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let mut phone = alone(&dir).await?;
    let me = phone.device_id();
    let milk = phone.add_task("Milk", None, &[]).await?;
    phone.assign_task(milk, Some(me)).await?;
    phone.start_task(milk).await?;
    phone.pause_task(milk).await?;
    ensure!(phone.state()?.tasks[&milk].progress() == Some(Progress::Paused));
    phone.assign_task(milk, None).await?;
    ensure!(phone.state()?.tasks[&milk].assignee.is_none(), "given back");

    phone.complete_task(milk, None).await?;
    let here = GeoPoint::new(48.8752, 9.3775, None)?;
    phone.locate_completion(milk, here).await?;
    ensure!(
        phone.state()?.tasks[&milk]
            .done
            .as_ref()
            .is_some_and(|done| done.location == Some(here))
    );
    phone.undo_completion(milk).await?;
    ensure!(!phone.state()?.tasks[&milk].is_done(), "undone");
    phone.complete_task(milk, None).await?;
    phone.reopen_task(milk).await?;
    ensure!(!phone.state()?.tasks[&milk].is_done(), "open again");

    phone.delete_task(milk).await?;
    ensure!(phone.state()?.tasks.is_empty(), "deleted");
    ensure!(phone.start_task(milk).await.is_err(), "it is gone");
    Ok(())
}

#[tokio::test]
async fn problems_are_noticed_handed_over_resolved_reopened_and_planned() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let mut phone = alone(&dir).await?;
    let me = phone.device_id();
    let food = phone
        .create_problem("Out of food", "kitchen", &["Go shopping"])
        .await?;
    phone.rename_problem(food, "Nothing to eat").await?;
    phone.add_solution(food, "Cook pasta").await?;
    let shopping = phone.state()?.problems[&food].solutions[0].id;
    phone.remove_solution(food, shopping).await?;
    let problem = &phone.state()?.problems[&food];
    ensure!(problem.title == "Nothing to eat");
    ensure!(problem.solutions.len() == 1 && problem.solutions[0].title.to_string() == "Cook pasta");

    phone.notice_problem(food, me).await?;
    ensure!(phone.state()?.problems[&food].holder() == Some(me), "mine");
    phone.assign_problem(food, me).await?;
    let step = phone
        .state()?
        .steps(food)
        .map(|task| task.id)
        .next()
        .context("a step")?;
    let today = chrono::Utc::now().date_naive();
    phone.plan_day(today, &[step]).await?;
    ensure!(
        phone.state()?.plan(me, today) == Some(&[step][..]),
        "planned"
    );

    phone.resolve_problem(food).await?;
    ensure!(!phone.state()?.problems[&food].open, "resolved");
    phone.reopen_problem(food).await?;
    ensure!(phone.state()?.problems[&food].open, "open again");

    ensure!(
        phone.notice_problem(Uuid::now_v7(), me).await.is_err(),
        "an unknown problem is not noticed"
    );
    ensure!(
        phone
            .create_problem("Bad", "Not An Icon", &[])
            .await
            .is_err(),
        "icons are checked"
    );
    ensure!(
        phone
            .create_problem("Bad", "kitchen", &[" "])
            .await
            .is_err(),
        "solutions are checked"
    );
    Ok(())
}

#[tokio::test]
async fn logging_out_forgets_the_family_and_its_log() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let mut phone = alone(&dir).await?;
    phone.add_task("Milk", None, &[]).await?;
    ensure!(phone.events()?.len() == 2, "created, and Milk");

    phone.logout()?;
    ensure!(phone.membership().is_none() && phone.events()?.is_empty());
    let again = Device::open(dir.path().join("phone"))?;
    ensure!(again.membership().is_none(), "forgotten on disk too");
    Ok(())
}
