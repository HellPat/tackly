//! Bringing a new phone into the family.
//!
//! The head creates an invitation: a one-time secret that never reaches the
//! relay, shown as a QR code and a link. The new phone presents a proof of the
//! secret and its MLS key package; both phones show the same six digits,
//! derived from the secret and that key package, and the head confirms them.
//! The head's phone then adds the new phone to the group (a commit for the
//! family, a welcome for the new phone, which the relay passes along) and
//! sends it the history as a snapshot.

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::time::Duration;

use tackly_protocol::{
    FamilyCommand,
    wire::{ApproveJoin, ClaimInvite, CreateInvite, InviteProof, InviteState, token_hash},
};
use uuid::Uuid;

use super::{Device, Membership};
use crate::{
    api::{Api, HttpError},
    crypto::{self, derive, random_bytes},
    mls::confirmation_code,
};

/// How long a new member waits for the head's history snapshot.
const SNAPSHOT_WAIT: Duration = Duration::from_secs(60);

/// What the owner shows the other phone. `link` is shown as a QR code and can
/// be copied and sent by any messenger; the other phone scans or pastes it.
#[derive(Clone, Debug)]
pub struct InviteTicket {
    pub invite_id: Uuid,
    pub link: String,
    secret: Vec<u8>,
    /// For an AI agent: how long it may read.
    pub agent: Option<chrono::Duration>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InviteProgress {
    Waiting,
    /// A phone asked to join. Compare `confirmation` with the one it shows.
    Requested {
        device_id: Uuid,
        confirmation: String,
    },
    Approved,
    Gone,
}

/// The joining phone's half-finished request.
#[derive(Clone, Debug)]
pub struct JoinRequest {
    pub confirmation: String,
    invite: InviteLink,
    secret: Vec<u8>,
    /// Chosen when asking, so that polling again after a lost answer is safe.
    device_token: String,
}

/// What the invitation link carries.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct InviteLink {
    server: String,
    family: Uuid,
    invite: Uuid,
    secret: String,
    /// The invitation is for an AI agent, not a phone.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    agent: bool,
}

/// What an AI agent is called in the family.
pub const AGENT_NAME: &str = "AI agent";

/// An invitation is the link `tackly://join?c=<payload>`.
const LINK_PREFIX: &str = "tackly://join?c=";

/// The payload of an invitation link. A bare payload is accepted too, and so
/// is a link that a messenger wrapped or padded with whitespace.
fn invitation_payload(text: &str) -> Option<String> {
    let compact: String = text.split_whitespace().collect();
    let payload = compact.strip_prefix(LINK_PREFIX).unwrap_or(&compact);
    // Drop anything a messenger may have appended after the payload.
    let payload = payload.split(['&', '#']).next()?;
    (!payload.is_empty()).then(|| payload.to_owned())
}

fn invite_verifier(secret: &[u8]) -> String {
    crypto::encode(&derive("invite-verifier", &[secret]))
}

impl Device {
    // ---- the owner's side ----------------------------------------------------------

    /// An invitation for a phone.
    pub async fn create_invite(&mut self) -> Result<InviteTicket> {
        self.invite(None).await
    }

    /// An invitation for an AI agent, which may read along for `duration`.
    pub async fn create_agent_invite(
        &mut self,
        duration: chrono::Duration,
    ) -> Result<InviteTicket> {
        ensure!(
            duration > chrono::Duration::zero(),
            "an agent needs some time"
        );
        self.invite(Some(duration)).await
    }

    async fn invite(&mut self, agent: Option<chrono::Duration>) -> Result<InviteTicket> {
        // The joiner must find the whole history on the relay.
        self.flush().await?;
        let membership = self.joined()?.clone();
        ensure!(membership.owner, "only the head of the family can invite");
        let secret = random_bytes::<32>().to_vec();
        let invite_id = Uuid::new_v4();
        self.api()?
            .create_invite(
                membership.family_id,
                &membership.device_token,
                &CreateInvite {
                    invite_id,
                    verifier: invite_verifier(&secret),
                    agent_seconds: agent.map(|duration| duration.num_seconds().max(1)),
                },
            )
            .await?;
        let payload = InviteLink {
            server: membership.server_url,
            family: membership.family_id,
            invite: invite_id,
            secret: crypto::encode(&secret),
            agent: agent.is_some(),
        };
        Ok(InviteTicket {
            invite_id,
            link: format!(
                "{LINK_PREFIX}{}",
                crypto::encode(&serde_json::to_vec(&payload)?)
            ),
            secret,
            agent,
        })
    }

    pub async fn invite_progress(&self, ticket: &InviteTicket) -> Result<InviteProgress> {
        let membership = self.joined()?;
        let status = match self
            .api()?
            .invite_status(
                membership.family_id,
                &membership.device_token,
                ticket.invite_id,
            )
            .await
        {
            Ok(status) => status,
            Err(error) if HttpError::is_gone(&error) => return Ok(InviteProgress::Gone),
            Err(error) => return Err(error),
        };
        Ok(match (status.status, status.pending_device_id) {
            (InviteState::Pending, Some(device_id)) => {
                let key_package = status
                    .pending_key_package
                    .as_deref()
                    .map(crypto::decode)
                    .transpose()?
                    .context("the relay sent no key package")?;
                InviteProgress::Requested {
                    device_id,
                    confirmation: confirmation_code(&ticket.secret, &key_package),
                }
            }
            (InviteState::Approved | InviteState::Used, _) => InviteProgress::Approved,
            (InviteState::Cancelled, _) => InviteProgress::Gone,
            _ => InviteProgress::Waiting,
        })
    }

    /// Call only after the person compared the confirmation digits. Adds the
    /// phone to the group, hands it the welcome, and sends it the history.
    pub async fn approve_join(&mut self, ticket: &InviteTicket, device_id: Uuid) -> Result<()> {
        let membership = self.joined()?.clone();
        let api = self.api()?;
        let status = api
            .invite_status(
                membership.family_id,
                &membership.device_token,
                ticket.invite_id,
            )
            .await?;
        ensure!(
            status.pending_device_id == Some(device_id),
            "that phone is no longer asking to join"
        );
        let key_package = crypto::decode(
            status
                .pending_key_package
                .as_deref()
                .context("the relay sent no key package")?,
        )?;
        // The joiner must find the whole history in the snapshot.
        self.flush().await?;
        let welcome = self.commit_add(device_id, &key_package).await?;
        api.approve_join(
            membership.family_id,
            &membership.device_token,
            ticket.invite_id,
            &ApproveJoin {
                device_id,
                welcome: crypto::encode(&welcome),
            },
        )
        .await?;
        if let Some(duration) = ticket.agent {
            // The family's record says who the agent is and until when; the
            // snapshot carries it to the agent (and the flush, to the others).
            self.run(FamilyCommand::AdmitAgent {
                member_id: device_id,
                name: AGENT_NAME.to_owned(),
                until: chrono::Utc::now() + duration,
            })
            .await?;
        }
        self.send_snapshot().await?;
        self.flush().await
    }

    /// Adds a member: the commit goes to the relay (again, after catching up,
    /// if the group moved on meanwhile); the welcome comes back for the member.
    async fn commit_add(&mut self, device_id: Uuid, key_package: &[u8]) -> Result<Vec<u8>> {
        let family_id = self.joined()?.family_id;
        let mut welcome = Vec::new();
        self.commit(|device| {
            let (commit, new_welcome) = device.mls.add_member(family_id, device_id, key_package)?;
            welcome = new_welcome;
            Ok(commit)
        })
        .await?;
        Ok(welcome)
    }

    // ---- the joining phone's side ---------------------------------------------------

    /// Step 1: ask the owner to let this phone (or, `as_agent`, this AI
    /// agent) in. The invitation must be meant for that kind of member.
    pub async fn request_join(&mut self, link: &str, as_agent: bool) -> Result<JoinRequest> {
        ensure!(self.membership.is_none(), "already in a family");
        let encoded = invitation_payload(link).context("not a Tackly invitation link")?;
        let invite: InviteLink = serde_json::from_slice(&crypto::decode(&encoded)?)
            .context("the invitation link is damaged")?;
        ensure!(
            invite.agent == as_agent,
            if invite.agent {
                "this invitation is for an AI agent"
            } else {
                "this invitation is for a phone"
            }
        );
        let secret = crypto::decode(&invite.secret)?;
        let key_package = self.mls.key_package()?;
        Api::new(&invite.server)?
            .request_join(
                invite.invite,
                &InviteProof {
                    verifier: invite_verifier(&secret),
                    device_id: self.device_id(),
                    key_package: crypto::encode(&key_package),
                },
            )
            .await
            .context("the invitation is expired or already used")?;
        Ok(JoinRequest {
            confirmation: confirmation_code(&secret, &key_package),
            invite,
            secret,
            device_token: crypto::encode(&random_bytes::<32>()),
        })
    }

    /// Step 2, polled until the owner approved: `Ok(false)` means not yet.
    pub async fn complete_join(&mut self, request: &JoinRequest, my_name: &str) -> Result<bool> {
        ensure!(!my_name.trim().is_empty(), "name is required");
        let api = Api::new(&request.invite.server)?;
        let claimed = match api
            .claim_invite(
                request.invite.invite,
                &ClaimInvite {
                    verifier: invite_verifier(&request.secret),
                    device_id: self.device_id(),
                    token_hash: token_hash(&request.device_token),
                },
            )
            .await
        {
            Ok(claimed) => claimed,
            Err(error) if HttpError::is_gone(&error) => return Ok(false),
            Err(error) => return Err(error),
        };
        ensure!(
            claimed.family_id == request.invite.family,
            "the invitation belongs to another family"
        );
        let (family_id, head) = self.mls.join(&crypto::decode(&claimed.welcome)?)?;
        ensure!(
            family_id == claimed.family_id,
            "the welcome is for another family"
        );
        let agent = request.invite.agent;
        self.save_membership(Membership {
            server_url: api.base().to_owned(),
            family_id,
            head,
            device_token: request.device_token.clone(),
            registered: true,
            owner: false,
            name: my_name.trim().to_owned(),
            agent,
            removed: false,
            restoring: false,
        })?;
        // The history comes from the head as a snapshot, right after the
        // welcome; wait for it, then announce ourselves (an agent says
        // nothing: the head announced it).
        self.events.lock().advance_cursor(claimed.after)?;
        let deadline = std::time::Instant::now() + SNAPSHOT_WAIT;
        loop {
            self.pull().await?;
            if self.state()?.family_id.is_some() {
                break;
            }
            ensure!(
                std::time::Instant::now() < deadline,
                "the head did not send the family's history"
            );
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        if agent {
            return Ok(true);
        }
        self.run(FamilyCommand::Join {
            name: my_name.to_owned(),
        })
        .await?;
        self.flush().await?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invitation_links_are_read_leniently() {
        let payload = "eyJzZXJ2ZXIiOiJodHRwOi8veCJ9";
        for text in [
            format!("tackly://join?c={payload}"),
            format!("  tackly://join?c={payload}\n"),
            "tackly://join?c=eyJzZXJ2\n  ZXIiOiJodHRwOi8veCJ9".to_owned(),
            payload.to_owned(),
            format!("tackly://join?c={payload}&utm=x"),
        ] {
            assert_eq!(
                invitation_payload(&text).as_deref(),
                Some(payload),
                "{text:?}"
            );
        }
        assert_eq!(invitation_payload("  "), None);
    }
}
