//! `tackly-agent`: an AI agent's way into a family. The agent logs in: it asks
//! the family for access and prints a code; the head of the family sees the
//! request on their phone, compares the code and allows access for a time.
//! From then on the agent reads: the family as it stands, the whole event
//! log, or every new event as it comes. It cannot write, and when its time is
//! over the head's phone takes it out.

use std::{collections::HashSet, path::PathBuf, time::Duration};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use tackly_phone::{Device, SharedDevice, infrastructure::server_url::ServerUrl, run_live_until};
use uuid::Uuid;

#[derive(Parser)]
#[command(name = "tackly-agent", version, about)]
struct Cli {
    /// Where the agent keeps its keys and its copy of the family.
    #[arg(long, env = "TACKLY_AGENT_DIR")]
    data_dir: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Ask a family for access. Prints a code for the head to compare with the
    /// request on their phone, then waits until the head decides.
    Login {
        /// The relay's address, as the family's Family screen shows it.
        #[arg(long, env = "TACKLY_SERVER")]
        server: ServerUrl,
        /// The family's ID, as the Family screen shows it.
        #[arg(long, env = "TACKLY_FAMILY")]
        family: Uuid,
    },
    /// The family as it stands, as JSON.
    State,
    /// The event log, one JSON event per line.
    Events,
    /// Every new event as it arrives, one JSON line each, until stopped.
    Follow,
    /// Forget the family and this agent's keys.
    Logout,
}

/// How often a waiting agent asks whether the head decided.
const LOGIN_POLL: Duration = Duration::from_secs(1);

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let dir = match cli.data_dir {
        Some(dir) => dir,
        None => PathBuf::from(std::env::var("HOME").context("no HOME")?).join(".tackly-agent"),
    };
    let mut device = Device::open(&dir)?;
    match cli.command {
        Command::Login { server, family } => {
            let access = device.ask_for_access(&server, family).await?;
            println!("Code: {}", access.code);
            println!("Waiting for the head of the family to compare it and allow access…");
            while !device.complete_access(&access).await? {
                tokio::time::sleep(LOGIN_POLL).await;
            }
            let family = device.state()?;
            println!(
                "In. Family: {}, {} people.",
                family.name.as_deref().unwrap_or("?"),
                family.people().count()
            );
        }
        Command::State => {
            catch_up(&mut device).await;
            println!("{}", serde_json::to_string_pretty(&device.state()?)?);
        }
        Command::Events => {
            catch_up(&mut device).await;
            for event in device.events()? {
                println!("{}", serde_json::to_string(&event)?);
            }
        }
        Command::Follow => follow(device).await?,
        Command::Logout => {
            device.logout()?;
            println!("Logged out.");
        }
    }
    Ok(())
}

/// Pulls what is new; without the relay, what is here is shown.
async fn catch_up(device: &mut Device) {
    if let Err(error) = device.pull().await {
        eprintln!("could not reach the relay, showing what is here: {error:#}");
    }
}

/// Prints every event once, as it arrives, until Ctrl-C.
async fn follow(device: Device) -> Result<()> {
    let device: SharedDevice = std::sync::Arc::new(tokio::sync::Mutex::new(device));
    let (changed, mut wake) = tokio::sync::mpsc::unbounded_channel::<()>();
    let live = tokio::spawn(run_live_until(
        device.clone(),
        move || {
            let _ = changed.send(());
        },
        std::future::pending(),
    ));
    let stop = tokio::signal::ctrl_c();
    tokio::pin!(stop);
    let mut printed = HashSet::new();
    loop {
        for event in device.lock().await.events()? {
            if printed.insert(event.id) {
                println!("{}", serde_json::to_string(&event)?);
            }
        }
        tokio::select! {
            Some(()) = wake.recv() => {}
            _ = &mut stop => break,
        }
    }
    live.abort();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_command_line_is_well_formed() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }
}
