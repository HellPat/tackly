//! `tackly-agent` as an agent uses it: the real program, against a real relay
//! (the Worker under `wrangler dev`), with the family's head on a phone that
//! allows it in.

use std::{path::Path, process::Stdio, time::Duration};

use anyhow::{Context, Result, ensure};
use tackly_phone::Device;
use tackly_testkit::Relay;
use tokio::{
    io::{AsyncBufReadExt, BufReader, Lines},
    process::{Child, ChildStdout, Command},
};

/// The agent program, keeping its data in `dir`.
fn agent(dir: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_tackly-agent"));
    command
        .arg("--data-dir")
        .arg(dir)
        .env_remove("TACKLY_SERVER")
        .env_remove("TACKLY_FAMILY")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

fn lines(child: &mut Child) -> Result<Lines<BufReader<ChildStdout>>> {
    Ok(BufReader::new(child.stdout.take().context("stdout")?).lines())
}

/// Reads lines until one contains `text`.
async fn line_with(lines: &mut Lines<BufReader<ChildStdout>>, text: &str) -> Result<String> {
    tokio::time::timeout(Duration::from_secs(30), async {
        while let Some(line) = lines.next_line().await? {
            if line.contains(text) {
                return Ok(line);
            }
        }
        anyhow::bail!("no line with {text:?}")
    })
    .await?
}

/// What a finished run printed, and whether it succeeded.
async fn run(command: &mut Command) -> Result<(bool, String, String)> {
    let output = command.output().await?;
    Ok((
        output.status.success(),
        String::from_utf8(output.stdout)?,
        String::from_utf8(output.stderr)?,
    ))
}

#[tokio::test]
async fn an_agent_logs_in_reads_follows_and_logs_out() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let mut relay = Relay::reserve(dir.path())?;
    relay.start().await?;
    let mut head = Device::open(dir.path().join("head"))?;
    head.create_family(&relay.url(), "The Smiths", "Patrick")
        .await?;
    head.add_task("Milk", None, &[]).await?;
    head.flush().await?;
    let family = head.membership().context("in a family")?.family_id;
    let me = dir.path().join("agent");

    // The agent asks; the head compares the code and allows it.
    let mut login = agent(&me)
        .args(["login", "--server", relay.url().as_str(), "--family"])
        .arg(family.to_string())
        .spawn()?;
    let mut said = lines(&mut login)?;
    let code = line_with(&mut said, "Code: ").await?;
    let ask = loop {
        if let Some(ask) = head.agent_asks().await?.into_iter().next() {
            break ask;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    };
    ensure!(code == format!("Code: {}", ask.code), "the same code");
    head.allow_agent(&ask, chrono::Duration::minutes(10))
        .await?;
    line_with(&mut said, "In. Family: The Smiths, 1 people.").await?;
    ensure!(login.wait().await?.success(), "logged in");

    // It reads the family and its log.
    let (ok, state, _) = run(agent(&me).arg("state")).await?;
    ensure!(ok && state.contains("\"Milk\""), "the state: {state}");
    let (ok, events, _) = run(agent(&me).arg("events")).await?;
    ensure!(ok && events.contains("task.created"), "the log: {events}");

    // It follows: what the head adds arrives, and Ctrl-C ends it.
    let mut follow = agent(&me).arg("follow").spawn()?;
    let mut followed = lines(&mut follow)?;
    line_with(&mut followed, "Milk").await?;
    head.add_task("Eggs", None, &[]).await?;
    head.flush().await?;
    line_with(&mut followed, "Eggs").await?;
    let pid = i32::try_from(follow.id().context("running")?)?;
    // SAFETY: sends a signal; no memory is involved.
    unsafe { libc::kill(pid, libc::SIGINT) };
    ensure!(follow.wait().await?.success(), "Ctrl-C ends it");

    // Without the relay it shows what it has.
    relay.stop();
    let (ok, state, complaint) = run(agent(&me).arg("state")).await?;
    ensure!(ok && state.contains("\"Eggs\""), "what it has: {state}");
    ensure!(
        complaint.contains("could not reach the relay"),
        "{complaint}"
    );

    let (ok, said, _) = run(agent(&me).arg("logout")).await?;
    ensure!(ok && said.contains("Logged out."), "{said}");
    Ok(())
}

#[tokio::test]
async fn without_a_folder_the_agent_keeps_its_data_at_home() -> Result<()> {
    let home = tempfile::tempdir()?;
    let mut logout = Command::new(env!("CARGO_BIN_EXE_tackly-agent"));
    logout
        .arg("logout")
        .env("HOME", home.path())
        .env_remove("TACKLY_AGENT_DIR");
    let (ok, said, _) = run(&mut logout).await?;
    ensure!(ok && said.contains("Logged out."), "{said}");
    ensure!(home.path().join(".tackly-agent").exists(), "kept at home");

    logout.env_remove("HOME");
    let (ok, _, complaint) = run(&mut logout).await?;
    ensure!(!ok && complaint.contains("no HOME"), "{complaint}");
    Ok(())
}
