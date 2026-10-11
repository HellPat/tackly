//! `tackly-agent`: an AI agent's way into a family. The head of the family
//! makes an invitation for an agent (with how long it may read), the agent
//! joins with it, and from then on it reads: the family as it stands, the
//! whole event log, or every new event as it comes. It cannot write, and
//! when its time is over the head's phone takes it out.

use std::{collections::HashSet, path::PathBuf, time::Duration};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use tackly_phone::{AGENT_NAME, Device, SharedDevice, run_live};

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
    /// Ask to join with an invitation link the head made for an AI agent.
    /// Prints the six digits for the head to compare, then waits to be let in.
    Join { link: String },
    /// The family as it stands, as JSON.
    State,
    /// The event log, one JSON event per line.
    Events,
    /// Every new event as it arrives, one JSON line each, until stopped.
    Follow,
    /// Forget the family and this agent's keys.
    Leave,
}

/// How often a waiting agent asks whether it was let in.
const JOIN_POLL: Duration = Duration::from_secs(1);

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let dir = match cli.data_dir {
        Some(dir) => dir,
        None => PathBuf::from(std::env::var("HOME").context("no HOME")?).join(".tackly-agent"),
    };
    let mut device = Device::open(&dir)?;
    match cli.command {
        Command::Join { link } => {
            let request = device.request_join(&link, true).await?;
            println!("Confirmation code: {}", request.confirmation);
            println!("Waiting for the head of the family to compare it and let the agent in…");
            while !device.complete_join(&request, AGENT_NAME).await? {
                tokio::time::sleep(JOIN_POLL).await;
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
        Command::Leave => {
            device.logout()?;
            println!("Left.");
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

/// Prints every event once, as it arrives.
async fn follow(device: Device) -> Result<()> {
    let device: SharedDevice = std::sync::Arc::new(tokio::sync::Mutex::new(device));
    let (changed, mut wake) = tokio::sync::mpsc::unbounded_channel::<()>();
    let live = tokio::spawn(run_live(device.clone(), move || {
        let _ = changed.send(());
    }));
    let mut printed = HashSet::new();
    loop {
        let events = device.lock().await.events()?;
        for event in events {
            if printed.insert(event.id) {
                println!("{}", serde_json::to_string(&event)?);
            }
        }
        if wake.recv().await.is_none() {
            break;
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
