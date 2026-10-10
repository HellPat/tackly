//! Typed HTTP client for the sync relay.

use anyhow::{Context, Result, bail};
use futures_util::StreamExt;
use reqwest::{Client, RequestBuilder, Response, StatusCode};
use serde::{Serialize, de::DeserializeOwned};
use tackly_protocol::wire::*;
use uuid::Uuid;

#[derive(Clone)]
pub struct Api {
    http: Client,
    base: String,
}

#[derive(Debug)]
pub struct HttpError(pub StatusCode, pub String);

impl std::fmt::Display for HttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "server answered {}: {}", self.0, self.1)
    }
}
impl std::error::Error for HttpError {}

async fn parse<T: DeserializeOwned>(response: Response) -> Result<T> {
    let status = response.status();
    if status.is_success() {
        return Ok(response.json().await?);
    }
    let message = response
        .json::<ApiErrorBody>()
        .await
        .map(|body| body.error)
        .unwrap_or_default();
    Err(HttpError(status, message).into())
}

impl Api {
    pub fn new(base: &str) -> Result<Self> {
        let base = base.trim().trim_end_matches('/').to_owned();
        if !(base.starts_with("http://") || base.starts_with("https://")) {
            bail!("server address must start with http:// or https://");
        }
        Ok(Self {
            http: Client::builder()
                .connect_timeout(std::time::Duration::from_secs(5))
                .build()?,
            base,
        })
    }

    pub fn base(&self) -> &str {
        &self.base
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    async fn send<T: DeserializeOwned>(&self, request: RequestBuilder) -> Result<T> {
        parse(
            request
                .timeout(std::time::Duration::from_secs(15))
                .send()
                .await?,
        )
        .await
    }

    fn post<B: Serialize>(&self, path: &str, token: Option<&str>, body: &B) -> RequestBuilder {
        let request = self.http.post(self.url(path)).json(body);
        match token {
            Some(token) => request.bearer_auth(token),
            None => request,
        }
    }

    pub async fn health(&self) -> Result<()> {
        let response = self.http.get(self.url("/health")).send().await?;
        if response.status() != StatusCode::NO_CONTENT {
            bail!("unexpected health status {}", response.status());
        }
        Ok(())
    }

    pub async fn create_family(&self, body: &CreateFamily) -> Result<DeviceToken> {
        self.send(self.post("/v1/families", None, body)).await
    }

    pub async fn append(
        &self,
        family: Uuid,
        token: &str,
        events: Vec<EncryptedEvent>,
    ) -> Result<AppendResult> {
        let path = format!("/v1/families/{family}/events");
        self.send(self.post(&path, Some(token), &AppendEvents { events }))
            .await
    }

    pub async fn events(&self, family: Uuid, token: &str, after: i64) -> Result<EventsPage> {
        let request = self
            .http
            .get(self.url(&format!("/v1/families/{family}/events")))
            .query(&[("after", after)])
            .bearer_auth(token);
        self.send(request).await
    }

    pub async fn create_invite(
        &self,
        family: Uuid,
        token: &str,
        body: &CreateInvite,
    ) -> Result<InviteExpiry> {
        self.send(self.post(&format!("/v1/families/{family}/invites"), Some(token), body))
            .await
    }

    pub async fn invite_status(
        &self,
        family: Uuid,
        token: &str,
        invite: Uuid,
    ) -> Result<InviteStatus> {
        let request = self
            .http
            .get(self.url(&format!("/v1/families/{family}/invites/{invite}")))
            .bearer_auth(token);
        self.send(request).await
    }

    pub async fn approve_join(
        &self,
        family: Uuid,
        token: &str,
        invite: Uuid,
        body: &ApproveJoin,
    ) -> Result<InviteStatus> {
        let path = format!("/v1/families/{family}/invites/{invite}/approve");
        self.send(self.post(&path, Some(token), body)).await
    }

    pub async fn cancel_invite(
        &self,
        family: Uuid,
        token: &str,
        invite: Uuid,
    ) -> Result<InviteStatus> {
        let request = self
            .http
            .delete(self.url(&format!("/v1/families/{family}/invites/{invite}")))
            .bearer_auth(token);
        self.send(request).await
    }

    pub async fn request_join(&self, invite: Uuid, body: &InviteProof) -> Result<InviteStatus> {
        self.send(self.post(&format!("/v1/invites/{invite}/request"), None, body))
            .await
    }

    pub async fn claim_invite(&self, invite: Uuid, body: &ClaimInvite) -> Result<ClaimedInvite> {
        self.send(self.post(&format!("/v1/invites/{invite}/claim"), None, body))
            .await
    }

    /// Opens the SSE stream and yields one page per `events` message. Ends
    /// with an error when the connection drops; the caller reconnects from
    /// its cursor.
    pub async fn stream<Fut: std::future::Future<Output = Result<()>>>(
        &self,
        family: Uuid,
        token: &str,
        after: i64,
        mut on_page: impl FnMut(EventsPage) -> Fut,
    ) -> Result<()> {
        let response = self
            .http
            .get(self.url(&format!("/v1/families/{family}/stream")))
            .query(&[("after", after)])
            .bearer_auth(token)
            .header("accept", "text/event-stream")
            .send()
            .await?;
        if !response.status().is_success() {
            let status = response.status();
            bail!(HttpError(status, String::new()));
        }
        let mut bytes = response.bytes_stream();
        let mut buffer = String::new();
        while let Some(chunk) = bytes.next().await {
            buffer.push_str(
                &String::from_utf8_lossy(&chunk.context("stream interrupted")?)
                    .replace("\r\n", "\n"),
            );
            while let Some(end) = buffer.find("\n\n") {
                let message: String = buffer.drain(..end + 2).collect();
                let (mut name, mut data) = (None, String::new());
                for line in message.lines() {
                    if let Some(value) = line.strip_prefix("event:") {
                        name = Some(value.trim().to_owned());
                    } else if let Some(value) = line.strip_prefix("data:") {
                        data.push_str(value.strip_prefix(' ').unwrap_or(value));
                    }
                }
                if name.as_deref() == Some(SSE_EVENTS) {
                    on_page(serde_json::from_str(&data).context("decode event page")?).await?;
                }
            }
        }
        bail!("event stream closed")
    }
}
