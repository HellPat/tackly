//! Tackly sync relay: keeps each family's encrypted events, hands them to the
//! family's phones by cursor, and pushes new ones over Server-Sent Events.
//!
//! - [`families`]: registering a family
//! - [`events`]: the encrypted event log
//! - [`invites`]: letting a new phone join
//! - [`auth`]: who is calling

mod auth;
mod db;
mod error;
mod events;
mod families;
mod invites;
mod live;

use axum::{
    Router,
    extract::DefaultBodyLimit,
    http::StatusCode,
    routing::{get, post},
};
use sqlx::SqlitePool;

pub use db::{connect, migrate};

const MAX_REQUEST_BYTES: usize = 10 * 1024 * 1024;

#[derive(Clone)]
struct AppState {
    db: SqlitePool,
    live: live::Live,
}

pub fn router(pool: SqlitePool) -> Router {
    Router::new()
        .route("/health", get(|| async { StatusCode::NO_CONTENT }))
        .route("/v1/families", post(families::create_family))
        .route(
            "/v1/families/{family_id}/events",
            get(events::list_events).post(events::append_events),
        )
        .route(
            "/v1/families/{family_id}/stream",
            get(events::stream_events),
        )
        .route(
            "/v1/families/{family_id}/invites",
            post(invites::create_invite),
        )
        .route(
            "/v1/families/{family_id}/invites/{invite_id}",
            get(invites::invite_status).delete(invites::cancel_invite),
        )
        .route(
            "/v1/families/{family_id}/invites/{invite_id}/approve",
            post(invites::approve_join),
        )
        .route(
            "/v1/invites/{invite_id}/request",
            post(invites::request_join),
        )
        .route("/v1/invites/{invite_id}/claim", post(invites::claim_invite))
        .layer(DefaultBodyLimit::max(MAX_REQUEST_BYTES))
        .with_state(AppState {
            db: pool,
            live: live::Live::default(),
        })
}
