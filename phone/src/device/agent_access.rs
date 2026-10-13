//! An AI agent gets into a family: it asks, the head allows it for a time.
//!
//! The agent knows the relay and the family's ID, nothing secret. It leaves
//! its MLS key package with the relay and prints a code (12 digits derived
//! from that key package). The head's phone shows the same code with the
//! request: if the head sees the code on the agent's screen, the request is
//! the agent's (a relay that swapped the key would show another code). The
//! head allows it for a time: its phone adds the agent to the family's group,
//! leaves the welcome, and sends the history. The agent is a member that only
//! reads. The same shape as the OAuth device flow (RFC 8628).

use anyhow::{Context, Result, ensure};
use tackly_domain::MemberCommand;
use tackly_wire::{
    AgentAccessRequest, AgentRequestState, ApproveAgent, Bytes, ClaimAgent, MAX_AGENT_SECONDS,
};
use uuid::Uuid;

use super::{Device, pairing::Entry};
use crate::infrastructure::{
    device_token::DeviceToken,
    digit_code::AgentCode,
    group_crypto::agent_code,
    relay_client::{Api, HttpError},
    server_url::ServerUrl,
};

/// What an AI agent is called in the family.
pub const AGENT_NAME: &str = "AI agent";

/// The agent's request, while it waits for the head.
#[derive(Clone, Debug)]
pub struct AgentAccess {
    /// Twelve digits the head compares with its own screen.
    pub code: AgentCode,
    server_url: ServerUrl,
    family_id: Uuid,
    request_id: Uuid,
    /// Chosen here, so that asking again after a lost answer is safe.
    device_token: DeviceToken,
}

/// A request the head's phone shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentAsk {
    pub request_id: Uuid,
    pub device_id: Uuid,
    /// Twelve digits, derived from the agent's key package.
    pub code: AgentCode,
}

impl Device {
    // ---- the agent's side ------------------------------------------------------------

    /// Step 1: ask a family for access. Needs the relay's address and the
    /// family's ID.
    pub async fn ask_for_access(
        &mut self,
        server_url: &ServerUrl,
        family_id: Uuid,
    ) -> Result<AgentAccess> {
        ensure!(self.membership.is_none(), "already in a family");
        let api = Api::new(server_url)?;
        let key_package = self.mls.key_package()?;
        let request_id = Uuid::new_v4();
        api.ask_agent_access(
            family_id,
            &AgentAccessRequest {
                request_id,
                device_id: self.device_id(),
                key_package: Bytes::from(key_package.as_slice()),
            },
        )
        .await
        .context("could not ask that family for access")?;
        Ok(AgentAccess {
            code: agent_code(&key_package),
            server_url: api.base().clone(),
            family_id,
            request_id,
            device_token: DeviceToken::generate(),
        })
    }

    /// Step 2, polled until the head decided: `Ok(false)` means not yet; an
    /// error means the head denied it or it ran out.
    pub async fn complete_access(&mut self, access: &AgentAccess) -> Result<bool> {
        let api = Api::new(&access.server_url)?;
        let claimed = match api
            .claim_agent_request(
                access.request_id,
                &ClaimAgent {
                    device_id: self.device_id(),
                    token_hash: access.device_token.hash(),
                },
            )
            .await
        {
            Ok(claimed) => claimed,
            Err(error) if HttpError::is_gone(&error) => {
                anyhow::bail!("the head did not allow it, or the request ran out")
            }
            Err(error) => return Err(error),
        };
        if claimed.status == AgentRequestState::Pending {
            return Ok(false);
        }
        let (welcome, after) = (
            claimed.welcome.context("the relay sent no welcome")?,
            claimed.after.context("the relay sent no start")?,
        );
        let (family_id, head) = self.mls.join(welcome.as_slice())?;
        ensure!(
            Some(family_id) == claimed.family_id && family_id == access.family_id,
            "the welcome is for another family"
        );
        self.enter_family(Entry {
            server_url: access.server_url.clone(),
            family_id,
            head,
            device_token: access.device_token.clone(),
            name: AGENT_NAME.parse()?,
            agent: true,
            after,
        })
        .await?;
        Ok(true)
    }

    // ---- the head's side -------------------------------------------------------------

    /// The requests waiting for this phone's person (the head).
    pub async fn agent_asks(&self) -> Result<Vec<AgentAsk>> {
        let membership = self.joined()?;
        if !membership.owner {
            return Ok(Vec::new());
        }
        let pending = self
            .api()?
            .pending_agent_requests(membership.family_id, &membership.device_token)
            .await?;
        pending
            .requests
            .into_iter()
            .map(|request| {
                Ok(AgentAsk {
                    request_id: request.request_id,
                    device_id: request.device_id,
                    code: agent_code(request.key_package.as_slice()),
                })
            })
            .collect()
    }

    /// Lets the agent in for `duration`: adds it to the group, leaves the
    /// welcome, and sends it the history.
    pub async fn allow_agent(&mut self, ask: &AgentAsk, duration: chrono::Duration) -> Result<()> {
        let membership = self.joined()?.clone();
        ensure!(membership.owner, "only the head of the family can do that");
        let seconds = duration.num_seconds();
        ensure!(
            (1..=MAX_AGENT_SECONDS).contains(&seconds),
            "an agent reads for a second to thirty days"
        );
        // What the head saw must be what it adds.
        let pending = self
            .api()?
            .pending_agent_requests(membership.family_id, &membership.device_token)
            .await?;
        let request = pending
            .requests
            .into_iter()
            .find(|request| request.request_id == ask.request_id)
            .context("that request is gone")?;
        let key_package = request.key_package.as_slice().to_vec();
        ensure!(
            request.device_id == ask.device_id && agent_code(&key_package) == ask.code,
            "the request changed: look at the code again"
        );
        self.flush().await?;
        let welcome = self.commit_add(ask.device_id, &key_package).await?;
        self.api()?
            .approve_agent_request(
                membership.family_id,
                &membership.device_token,
                ask.request_id,
                &ApproveAgent {
                    agent_seconds: seconds,
                    welcome: Bytes::from(welcome),
                },
            )
            .await?;
        // The family's record says who the agent is and until when; the
        // snapshot carries it to the agent (and the flush, to the others).
        self.run(MemberCommand::AdmitAgent {
            member_id: ask.device_id,
            name: AGENT_NAME.parse()?,
            until: chrono::Utc::now() + duration,
        })
        .await?;
        self.send_snapshot().await?;
        self.flush().await
    }

    pub async fn deny_agent(&self, ask: &AgentAsk) -> Result<()> {
        let membership = self.joined()?;
        ensure!(membership.owner, "only the head of the family can do that");
        self.api()?
            .deny_agent_request(
                membership.family_id,
                &membership.device_token,
                ask.request_id,
            )
            .await
    }
}
