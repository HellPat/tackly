//! Bringing a new phone into the family.
//!
//! The owner creates an invitation: a one-time secret that never reaches the
//! relay, shown as a QR code and a link. The new phone presents a proof of the
//! secret; both phones show the same six digits, derived from the secret and
//! the new phone's ID, and the owner confirms them. The owner's phone then
//! seals the family key under the secret, and the relay only passes that
//! package along.

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use tackly_protocol::{
    FamilyCommand,
    wire::{ApproveJoin, ClaimInvite, CreateInvite, InviteProof, InviteState, token_hash},
};
use uuid::Uuid;

use super::{Device, Membership};
use crate::{
    api::{Api, HttpError},
    crypto::{self, SealedData, derive, random_bytes},
};

/// What the owner shows the other phone. `link` is shown as a QR code and can
/// be copied and sent by any messenger; the other phone scans or pastes it.
#[derive(Clone, Debug)]
pub struct InviteTicket {
    pub invite_id: Uuid,
    pub link: String,
    secret: Vec<u8>,
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
}

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

/// Six digits both phones can compare out loud.
fn confirmation_code(secret: &[u8], device_id: Uuid) -> String {
    let hash = derive("confirmation", &[secret, device_id.as_bytes()]);
    let number = u32::from_be_bytes([hash[0], hash[1], hash[2], hash[3]]) % 1_000_000;
    format!("{number:06}")
}

fn invite_verifier(secret: &[u8]) -> String {
    crypto::encode(&derive("invite-verifier", &[secret]))
}

/// Where the sealed family key is bound to: this invitation.
fn package_aad(invite_id: Uuid) -> String {
    format!("invite:{invite_id}")
}

impl Device {
    // ---- the owner's side ----------------------------------------------------------

    pub async fn create_invite(&mut self) -> Result<InviteTicket> {
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
                },
            )
            .await?;
        let payload = InviteLink {
            server: membership.server_url,
            family: membership.family_id,
            invite: invite_id,
            secret: crypto::encode(&secret),
        };
        Ok(InviteTicket {
            invite_id,
            link: format!(
                "{LINK_PREFIX}{}",
                crypto::encode(&serde_json::to_vec(&payload)?)
            ),
            secret,
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
            (InviteState::Pending, Some(device_id)) => InviteProgress::Requested {
                device_id,
                confirmation: confirmation_code(&ticket.secret, device_id),
            },
            (InviteState::Approved | InviteState::Used, _) => InviteProgress::Approved,
            (InviteState::Cancelled, _) => InviteProgress::Gone,
            _ => InviteProgress::Waiting,
        })
    }

    /// Call only after the person compared the confirmation digits.
    pub async fn approve_join(&self, ticket: &InviteTicket, device_id: Uuid) -> Result<()> {
        let membership = self.joined()?;
        let key = derive("invite-key", &[&ticket.secret]);
        let sealed = crypto::seal(&key, &self.family_key()?, &package_aad(ticket.invite_id))?;
        self.api()?
            .approve_join(
                membership.family_id,
                &membership.device_token,
                ticket.invite_id,
                &ApproveJoin {
                    device_id,
                    package_nonce: sealed.nonce,
                    package_ciphertext: sealed.ciphertext,
                    package_mac: sealed.mac,
                },
            )
            .await?;
        Ok(())
    }

    // ---- the joining phone's side ---------------------------------------------------

    /// Step 1: ask the owner to let this phone in.
    pub async fn request_join(&mut self, link: &str) -> Result<JoinRequest> {
        ensure!(self.membership.is_none(), "already in a family");
        let encoded = invitation_payload(link).context("not a Tackly invitation link")?;
        let invite: InviteLink = serde_json::from_slice(&crypto::decode(&encoded)?)
            .context("the invitation link is damaged")?;
        let secret = crypto::decode(&invite.secret)?;
        Api::new(&invite.server)?
            .request_join(
                invite.invite,
                &InviteProof {
                    verifier: invite_verifier(&secret),
                    device_id: self.device_id(),
                },
            )
            .await
            .context("the invitation is expired or already used")?;
        Ok(JoinRequest {
            confirmation: confirmation_code(&secret, self.device_id()),
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
        let family_key = crypto::open(
            &derive("invite-key", &[&request.secret]),
            &SealedData {
                nonce: claimed.package_nonce,
                ciphertext: claimed.package_ciphertext,
                mac: claimed.package_mac,
            },
            &package_aad(request.invite.invite),
        )?;
        self.save_membership(Membership {
            server_url: api.base().to_owned(),
            family_id: claimed.family_id,
            family_key: crypto::encode(&family_key),
            device_token: request.device_token.clone(),
            registered: true,
            owner: false,
            name: my_name.trim().to_owned(),
        })?;
        // Learn the family's history first, then announce ourselves.
        self.pull().await?;
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

    #[test]
    fn both_phones_derive_the_same_digits() {
        let (secret, device) = (b"secret".as_slice(), Uuid::now_v7());
        let digits = confirmation_code(secret, device);
        assert_eq!(digits.len(), 6);
        assert_eq!(digits, confirmation_code(secret, device));
        assert_ne!(digits, confirmation_code(secret, Uuid::now_v7()));
    }
}
