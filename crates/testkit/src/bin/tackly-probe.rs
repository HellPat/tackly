//! A phone without a screen, for tests that drive a real app from outside
//! (the Android spike). It is the head of a family on a relay, and answers one
//! JSON command per line on stdin with one JSON line on stdout:
//!
//! - `{"cmd":"invite"}` → `{"link":"tackly://join?c=…"}`
//! - `{"cmd":"approve"}` → waits until someone asks to join, lets them in → `{"code":"123456"}`
//! - `{"cmd":"state"}` → the family as this phone sees it (decrypted), as JSON
//!
//! `tackly-probe <data-dir> <relay-url>`

use std::{sync::Arc, time::Duration};

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use tackly_client::{Device, InviteProgress, InviteTicket, run_live};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    sync::Mutex,
};

#[tokio::main]
async fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let (dir, relay) = (
        args.next().context("data dir")?,
        args.next().context("relay url")?,
    );
    let device = Arc::new(Mutex::new(Device::open(&dir)?));
    device
        .lock()
        .await
        .create_family(&relay, "The Smiths", "Patrick")
        .await?;
    tokio::spawn(run_live(device.clone(), || {}));

    let mut ticket: Option<InviteTicket> = None;
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut out = tokio::io::stdout();
    while let Some(line) = lines.next_line().await? {
        let command: Value = serde_json::from_str(&line)?;
        let answer = match command["cmd"].as_str() {
            Some("invite") => {
                let invite = device.lock().await.create_invite().await?;
                let link = invite.link.clone();
                ticket = Some(invite);
                json!({ "link": link })
            }
            Some("approve") => {
                let invite = ticket.as_ref().context("invite first")?;
                json!({ "code": approve(&device, invite).await? })
            }
            Some("state") => serde_json::to_value(device.lock().await.state()?)?,
            other => bail!("unknown command {other:?}"),
        };
        out.write_all(format!("{answer}\n").as_bytes()).await?;
        out.flush().await?;
    }
    Ok(())
}

/// Waits (up to two minutes) for someone to ask to join, and lets them in.
async fn approve(device: &Mutex<Device>, invite: &InviteTicket) -> Result<String> {
    for _ in 0..240 {
        let progress = device.lock().await.invite_progress(invite).await?;
        if let InviteProgress::Requested {
            device_id,
            confirmation,
        } = progress
        {
            device.lock().await.approve_join(invite, device_id).await?;
            return Ok(confirmation);
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    bail!("nobody asked to join")
}
