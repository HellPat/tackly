//! The Tackly relay on Cloudflare: every request goes to one Durable Object
//! ([`relay::Relay`]), which keeps the families' data in D1 and holds the
//! phones' live connections.
//!
//! - [`families`]: registering a family
//! - [`events`]: the encrypted message log
//! - [`live`]: following the log over a WebSocket
//! - [`invites`]: letting a new phone join
//! - [`agents`]: an AI agent asks for access, the head allows it
//! - [`recovery`]: the head's sealed backup
//! - [`auth`]: who is calling, taking a device out, moving to a new token
//! - [`schema`]: the tables in D1

mod agents;
mod auth;
mod db;
mod error;
mod events;
mod families;
mod invites;
mod live;
mod recovery;
mod relay;
mod schema;

use worker::{Env, Request, Response, Result, event};

/// Where the one Durable Object lives: next to the D1 database.
const LOCATION: &str = "weur";

#[event(fetch)]
async fn fetch(request: Request, env: Env, _ctx: worker::Context) -> Result<Response> {
    let relay = env
        .durable_object("RELAY")?
        .id_from_name("relay")?
        .get_stub_with_location_hint(LOCATION)?;
    let response = relay.fetch_with_request(request).await?;
    if response.status_code() == 101 {
        return Ok(response);
    }
    // Which commit is live (CI waits for its own: scripts/deploy-relay.sh),
    // and nothing the relay answers may be kept by a cache on the way.
    let commit = env
        .var("TACKLY_COMMIT")
        .map_or_else(|_| "unknown".to_owned(), |commit| commit.to_string());
    let headers = response.headers().clone();
    headers.set("x-tackly-commit", &commit)?;
    headers.set("cache-control", "no-store")?;
    Ok(response.with_headers(headers))
}
