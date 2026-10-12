//! Typed HTTP client for the sync relay.

use anyhow::{Result, bail};
use reqwest::{Client, RequestBuilder, Response, StatusCode};
use reqwest_eventsource::EventSource;
use serde::{Serialize, de::DeserializeOwned};
use tackly_wire::*;
use uuid::Uuid;

use super::{device_token::DeviceToken, server_url::ServerUrl};

#[derive(Clone)]
pub struct Api {
    http: Client,
    base: ServerUrl,
}

#[derive(Debug)]
pub struct HttpError(pub StatusCode, pub String);

impl std::fmt::Display for HttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "server answered {}: {}", self.0, self.1)
    }
}
impl std::error::Error for HttpError {}

impl HttpError {
    /// 410: an invitation that expired, was used or was cancelled.
    pub fn is_gone(error: &anyhow::Error) -> bool {
        matches!(error.downcast_ref::<Self>(), Some(Self(status, _)) if status.as_u16() == 410)
    }

    /// 409 with [`STALE_EPOCH`]: the group moved on; catch up and encrypt again.
    pub fn is_stale_epoch(error: &anyhow::Error) -> bool {
        matches!(error.downcast_ref::<Self>(), Some(Self(status, text)) if status.as_u16() == 409 && text == STALE_EPOCH)
    }
}

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
    pub fn new(base: &ServerUrl) -> Result<Self> {
        Ok(Self {
            http: Client::builder()
                .connect_timeout(std::time::Duration::from_secs(5))
                .build()?,
            base: base.clone(),
        })
    }

    pub fn base(&self) -> &ServerUrl {
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

    fn post<B: Serialize>(
        &self,
        path: &str,
        token: Option<&DeviceToken>,
        body: &B,
    ) -> RequestBuilder {
        let request = self.http.post(self.url(path)).json(body);
        match token {
            Some(token) => request.bearer_auth(token.as_str()),
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

    /// Registers the family. Repeating it with the same token is harmless.
    pub async fn create_family(
        &self,
        family_id: Uuid,
        device_id: Uuid,
        token: &DeviceToken,
    ) -> Result<()> {
        let body = CreateFamily {
            family_id,
            device_id,
            token_hash: token.hash(),
        };
        self.send::<serde_json::Value>(self.post("/v1/families", None, &body))
            .await?;
        Ok(())
    }

    pub async fn append(
        &self,
        family: Uuid,
        token: &DeviceToken,
        events: Vec<EncryptedEvent>,
    ) -> Result<AppendResult> {
        let path = format!("/v1/families/{family}/events");
        self.send(self.post(&path, Some(token), &AppendEvents { events }))
            .await
    }

    pub async fn events(
        &self,
        family: Uuid,
        token: &DeviceToken,
        after: i64,
    ) -> Result<EventsPage> {
        let request = self
            .http
            .get(self.url(&format!("/v1/families/{family}/events")))
            .query(&[("after", after)])
            .bearer_auth(token.as_str());
        self.send(request).await
    }

    pub async fn create_invite(
        &self,
        family: Uuid,
        token: &DeviceToken,
        body: &CreateInvite,
    ) -> Result<InviteExpiry> {
        self.send(self.post(&format!("/v1/families/{family}/invites"), Some(token), body))
            .await
    }

    pub async fn invite_status(
        &self,
        family: Uuid,
        token: &DeviceToken,
        invite: Uuid,
    ) -> Result<InviteStatus> {
        let request = self
            .http
            .get(self.url(&format!("/v1/families/{family}/invites/{invite}")))
            .bearer_auth(token.as_str());
        self.send(request).await
    }

    pub async fn approve_join(
        &self,
        family: Uuid,
        token: &DeviceToken,
        invite: Uuid,
        body: &ApproveJoin,
    ) -> Result<InviteStatus> {
        let path = format!("/v1/families/{family}/invites/{invite}/approve");
        self.send(self.post(&path, Some(token), body)).await
    }

    pub async fn cancel_invite(
        &self,
        family: Uuid,
        token: &DeviceToken,
        invite: Uuid,
    ) -> Result<InviteStatus> {
        let request = self
            .http
            .delete(self.url(&format!("/v1/families/{family}/invites/{invite}")))
            .bearer_auth(token.as_str());
        self.send(request).await
    }

    /// The head takes a device's access to the relay away.
    pub async fn revoke_device(
        &self,
        family: Uuid,
        token: &DeviceToken,
        device: Uuid,
    ) -> Result<()> {
        let request = self
            .http
            .delete(self.url(&format!("/v1/families/{family}/devices/{device}")))
            .bearer_auth(token.as_str());
        self.send::<serde_json::Value>(request).await?;
        Ok(())
    }

    /// This device moves to a new token.
    pub async fn rotate_token(
        &self,
        family: Uuid,
        token: &DeviceToken,
        new_token: &DeviceToken,
    ) -> Result<()> {
        let path = format!("/v1/families/{family}/token");
        let body = RotateToken {
            token_hash: new_token.hash(),
        };
        self.send::<serde_json::Value>(self.post(&path, Some(token), &body))
            .await?;
        Ok(())
    }

    /// Leaves the head's sealed recovery backup with the relay.
    pub async fn put_recovery(
        &self,
        family: Uuid,
        token: &DeviceToken,
        id: &Digest,
        blob: &[u8],
    ) -> Result<()> {
        let request = self
            .http
            .put(self.url(&format!("/v1/families/{family}/recovery/{id}")))
            .bearer_auth(token.as_str())
            .json(&RecoveryBlob {
                blob: Bytes::from(blob),
            });
        self.send::<serde_json::Value>(request).await?;
        Ok(())
    }

    pub async fn delete_recovery(
        &self,
        family: Uuid,
        token: &DeviceToken,
        id: &Digest,
    ) -> Result<()> {
        let request = self
            .http
            .delete(self.url(&format!("/v1/families/{family}/recovery/{id}")))
            .bearer_auth(token.as_str());
        self.send::<serde_json::Value>(request).await?;
        Ok(())
    }

    /// The backup, for a phone that knows the recovery code (and so the ID).
    pub async fn get_recovery(&self, id: &Digest) -> Result<Vec<u8>> {
        let request = self.http.get(self.url(&format!("/v1/recovery/{id}")));
        let blob: RecoveryBlob = self.send(request).await?;
        Ok(blob.blob.into_vec())
    }

    /// An AI agent asks the family for access.
    pub async fn ask_agent_access(&self, family: Uuid, body: &AgentAccessRequest) -> Result<()> {
        self.send::<serde_json::Value>(self.post(
            &format!("/v1/families/{family}/agent-requests"),
            None,
            body,
        ))
        .await?;
        Ok(())
    }

    /// What waits for the head.
    pub async fn pending_agent_requests(
        &self,
        family: Uuid,
        token: &DeviceToken,
    ) -> Result<PendingAgentRequests> {
        let request = self
            .http
            .get(self.url(&format!("/v1/families/{family}/agent-requests")))
            .bearer_auth(token.as_str());
        self.send(request).await
    }

    pub async fn approve_agent_request(
        &self,
        family: Uuid,
        token: &DeviceToken,
        request: Uuid,
        body: &ApproveAgent,
    ) -> Result<()> {
        let path = format!("/v1/families/{family}/agent-requests/{request}/approve");
        self.send::<serde_json::Value>(self.post(&path, Some(token), body))
            .await?;
        Ok(())
    }

    pub async fn deny_agent_request(
        &self,
        family: Uuid,
        token: &DeviceToken,
        request: Uuid,
    ) -> Result<()> {
        let path = format!("/v1/families/{family}/agent-requests/{request}/deny");
        self.send::<serde_json::Value>(self.post(&path, Some(token), &serde_json::json!({})))
            .await?;
        Ok(())
    }

    pub async fn claim_agent_request(
        &self,
        request: Uuid,
        body: &ClaimAgent,
    ) -> Result<AgentClaim> {
        self.send(self.post(&format!("/v1/agent-requests/{request}/claim"), None, body))
            .await
    }

    pub async fn request_join(&self, invite: Uuid, body: &InviteProof) -> Result<InviteStatus> {
        self.send(self.post(&format!("/v1/invites/{invite}/request"), None, body))
            .await
    }

    pub async fn claim_invite(&self, invite: Uuid, body: &ClaimInvite) -> Result<ClaimedInvite> {
        self.send(self.post(&format!("/v1/invites/{invite}/claim"), None, body))
            .await
    }

    /// A resumable Server-Sent Events subscription. `reqwest-eventsource`
    /// parses the stream and, after a drop, reconnects with backoff and sends
    /// the last event ID it saw. A bad status (for example a revoked token)
    /// ends the stream instead of retrying.
    pub fn event_source(
        &self,
        family: Uuid,
        token: &DeviceToken,
        after: i64,
    ) -> Result<EventSource> {
        let request = self
            .http
            .get(self.url(&format!("/v1/families/{family}/stream")))
            .query(&[("after", after)])
            .bearer_auth(token.as_str());
        Ok(EventSource::new(request)?)
    }
}
