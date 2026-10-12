//! The Durable Object every request goes to. It exists once, so it is the one
//! place where the families' live connections meet the writes that should
//! wake them, and it lets one request run at a time: a handler reads D1,
//! decides, and writes one batch with nothing in between.

use std::{cell::Cell, str::FromStr};

use futures_util::lock::Mutex;
use serde::{Serialize, de::DeserializeOwned};
use uuid::Uuid;
use worker::{
    DurableObject, Env, Method, Request, Response, Result, State, WebSocket,
    WebSocketIncomingMessage, durable_object,
};

use crate::{
    agents, auth,
    db::Db,
    error::{ApiError, ApiResult, bad_request, internal, not_found},
    events, families, invites, live, recovery, schema,
};

/// The largest request body: a batch of messages, a welcome, a backup.
const MAX_REQUEST_BYTES: usize = 10 * 1024 * 1024;

#[durable_object]
pub struct Relay {
    state: State,
    env: Env,
    /// One request at a time (see the module comment).
    gate: Mutex<()>,
    /// The schema is up to date (checked once per wake-up).
    migrated: Cell<bool>,
}

impl DurableObject for Relay {
    fn new(state: State, env: Env) -> Self {
        live::answer_pings(&state);
        Self {
            state,
            env,
            gate: Mutex::new(()),
            migrated: Cell::new(false),
        }
    }

    async fn fetch(&self, request: Request) -> Result<Response> {
        let _turn = self.gate.lock().await;
        match self.serve(request).await {
            Ok(response) => Ok(response),
            Err(error) => error.into_response(),
        }
    }

    async fn websocket_message(
        &self,
        _ws: WebSocket,
        _message: WebSocketIncomingMessage,
    ) -> Result<()> {
        // Phones only listen; their pings are answered without waking us.
        Ok(())
    }

    async fn websocket_close(
        &self,
        _ws: WebSocket,
        _code: usize,
        _reason: String,
        _clean: bool,
    ) -> Result<()> {
        Ok(())
    }

    async fn websocket_error(&self, _ws: WebSocket, _error: worker::Error) -> Result<()> {
        Ok(())
    }
}

/// One request, with what its handler needs.
pub struct Call<'a> {
    pub db: &'a Db,
    pub state: &'a State,
    pub request: Request,
}

impl Call<'_> {
    /// The device token from `Authorization: Bearer …`.
    pub fn bearer(&self) -> Option<String> {
        let value = self.request.headers().get("authorization").ok()??;
        value.strip_prefix("Bearer ").map(str::to_owned)
    }

    pub fn query(&self, name: &str) -> Option<String> {
        let url = self.request.url().ok()?;
        url.query_pairs()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.into_owned())
    }

    /// The JSON body, at most [`MAX_REQUEST_BYTES`].
    pub async fn body<T: DeserializeOwned>(&mut self) -> Result<T, ApiError> {
        let bytes = self
            .request
            .bytes()
            .await
            .map_err(|_| bad_request("unreadable body"))?;
        if bytes.len() > MAX_REQUEST_BYTES {
            return Err(ApiError(413, "request too large"));
        }
        serde_json::from_slice(&bytes).map_err(|_| ApiError(422, "invalid body"))
    }
}

/// A JSON answer.
pub fn json<T: Serialize>(value: &T) -> ApiResult {
    Response::from_json(value).map_err(internal)
}

/// A path segment as an ID.
fn id<T: FromStr>(segment: &str) -> Result<T, ApiError> {
    segment.parse().map_err(|_| bad_request("invalid path"))
}

impl Relay {
    async fn serve(&self, request: Request) -> ApiResult {
        let db = Db(self.env.d1("DB").map_err(internal)?);
        if !self.migrated.get() {
            schema::migrate(&db).await?;
            self.migrated.set(true);
        }
        let method = request.method();
        let path = request.path();
        let segments: Vec<&str> = path.trim_matches('/').split('/').collect();
        let mut call = Call {
            db: &db,
            state: &self.state,
            request,
        };
        let call = &mut call;
        use Method::{Delete, Get, Post, Put};
        match (method, segments.as_slice()) {
            (Get, ["health"]) => Ok(Response::empty().map_err(internal)?.with_status(204)),
            (Post, ["v1", "families"]) => families::create(call).await,
            (Get, ["v1", "families", family, "events"]) => events::list(call, id(family)?).await,
            (Post, ["v1", "families", family, "events"]) => events::append(call, id(family)?).await,
            (Get, ["v1", "families", family, "stream"]) => live::connect(call, id(family)?).await,
            (Delete, ["v1", "families", family, "devices", device]) => {
                auth::revoke(call, id(family)?, id(device)?).await
            }
            (Post, ["v1", "families", family, "token"]) => auth::rotate(call, id(family)?).await,
            (Get, ["v1", "families", family, "agent-requests"]) => {
                agents::pending(call, id(family)?).await
            }
            (Post, ["v1", "families", family, "agent-requests"]) => {
                agents::ask(call, id(family)?).await
            }
            (
                Post,
                [
                    "v1",
                    "families",
                    family,
                    "agent-requests",
                    request,
                    "approve",
                ],
            ) => agents::approve(call, id(family)?, id(request)?).await,
            (Post, ["v1", "families", family, "agent-requests", request, "deny"]) => {
                agents::deny(call, id(family)?, id(request)?).await
            }
            (Post, ["v1", "agent-requests", request, "claim"]) => {
                agents::claim(call, id(request)?).await
            }
            (Put, ["v1", "families", family, "recovery", backup]) => {
                recovery::put(call, id(family)?, id(backup)?).await
            }
            (Delete, ["v1", "families", family, "recovery", backup]) => {
                recovery::delete(call, id(family)?, id(backup)?).await
            }
            (Get, ["v1", "recovery", backup]) => recovery::get(call, id(backup)?).await,
            (Post, ["v1", "families", family, "invites"]) => {
                invites::create(call, id(family)?).await
            }
            (Get, ["v1", "families", family, "invites", invite]) => {
                invites::status(call, id(family)?, id(invite)?).await
            }
            (Delete, ["v1", "families", family, "invites", invite]) => {
                invites::cancel(call, id(family)?, id(invite)?).await
            }
            (Post, ["v1", "families", family, "invites", invite, "approve"]) => {
                invites::approve(call, id(family)?, id(invite)?).await
            }
            (Post, ["v1", "invites", invite, "request"]) => {
                invites::request(call, id(invite)?).await
            }
            (Post, ["v1", "invites", invite, "claim"]) => invites::claim(call, id(invite)?).await,
            _ => Err(not_found("no such route")),
        }
    }
}

/// A UUID read back from D1.
pub fn stored_uuid(text: &str) -> Result<Uuid, ApiError> {
    Uuid::parse_str(text).map_err(internal)
}
