//! Live connections: a phone opens a WebSocket for its family and gets every
//! message after its cursor, then each new one as it is stored. The sockets
//! are hibernatable: the Durable Object may sleep while they stay open, and
//! each socket remembers its family and cursor itself.
//!
//! Messages to the phone are [`EventsPage`]s as JSON text. The phone sends
//! `ping` now and then; the runtime answers `pong` without waking anyone.

use serde::{Deserialize, Serialize};
use tackly_wire::MAX_PAGE;
use uuid::Uuid;
use worker::{Response, State, WebSocket, WebSocketPair, worker_sys::WebSocketRequestResponsePair};

use crate::{
    auth::Device,
    error::{ApiError, ApiResult, internal},
    events::{cursor, page_after},
    relay::Call,
};

/// What a socket remembers across hibernation.
#[derive(Serialize, Deserialize)]
struct Follower {
    family_id: Uuid,
    /// The last sequence sent on this socket.
    after: i64,
}

/// Lets the runtime answer the phones' pings while the object sleeps.
pub fn answer_pings(state: &State) {
    if let Ok(pair) = WebSocketRequestResponsePair::new("ping", "pong") {
        state.set_websocket_auto_response(&pair);
    }
}

/// `GET /v1/families/{id}/stream?after=` with `Upgrade: websocket`.
pub async fn connect(call: &mut Call<'_>, family_id: Uuid) -> ApiResult {
    let upgrade = call.request.headers().get("upgrade").ok().flatten();
    if !upgrade.is_some_and(|value| value.eq_ignore_ascii_case("websocket")) {
        return Err(ApiError(426, "a WebSocket is expected"));
    }
    Device::authenticate(call, family_id).await?;
    let after = cursor(call.query("after"))?.unwrap_or(0);
    let pair = WebSocketPair::new().map_err(internal)?;
    call.state
        .accept_websocket_with_tags(&pair.server, &[&family_id.to_string()]);
    pair.server
        .serialize_attachment(Follower { family_id, after })
        .map_err(internal)?;
    push(call, &pair.server).await?;
    Response::from_websocket(pair.client).map_err(internal)
}

/// Sends a socket everything after its cursor, a page at a time.
async fn push(call: &Call<'_>, socket: &WebSocket) -> Result<(), ApiError> {
    let Some(mut follower) = socket
        .deserialize_attachment::<Follower>()
        .map_err(internal)?
    else {
        return Ok(());
    };
    loop {
        let page = page_after(call.db, follower.family_id, follower.after, MAX_PAGE).await?;
        let Some(last) = page.events.last() else {
            return Ok(());
        };
        follower.after = last.sequence;
        let full = page.events.len() as i64 == MAX_PAGE;
        let text = serde_json::to_string(&page).map_err(internal)?;
        socket.send_with_str(text).map_err(internal)?;
        socket.serialize_attachment(&follower).map_err(internal)?;
        if !full {
            return Ok(());
        }
    }
}

/// New messages for `family_id` are stored: every open socket of the family
/// gets them. A socket that fails is left to its phone, which reconnects.
pub async fn wake(call: &Call<'_>, family_id: Uuid) {
    for socket in call.state.get_websockets_with_tag(&family_id.to_string()) {
        if push(call, &socket).await.is_err() {
            let _ = socket.close(Some(1011), Some("could not send"));
        }
    }
}
