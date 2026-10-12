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
use std::time::Duration;

use tackly_domain::{FamilyCommand, Name};
use tackly_wire::{
    ApproveJoin, Bytes, ClaimInvite, CreateInvite, Digest, InviteProof, InviteState,
};
use uuid::Uuid;

use super::{Device, InviteLink, Membership};
use crate::{
    infrastructure::crypto::{derive, random_bytes},
    infrastructure::device_token::DeviceToken,
    infrastructure::relay_client::{Api, HttpError},
    infrastructure::server_url::ServerUrl,
    infrastructure::{digit_code::ConfirmationCode, group_crypto::confirmation_code},
};

/// How long a new member waits for the head's history snapshot.
const SNAPSHOT_WAIT: Duration = Duration::from_secs(60);

/// What the owner shows the other phone. `link` is shown as a QR code and can
/// be copied and sent by any messenger; the other phone scans or pastes it.
#[derive(Clone, Debug)]
pub struct InviteTicket {
    pub invite_id: Uuid,
    /// Holds the secret, and keeps it out of logs.
    pub link: InviteLink,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InviteProgress {
    Waiting,
    /// A phone asked to join. Compare `confirmation` with the one it shows.
    Requested {
        device_id: Uuid,
        confirmation: ConfirmationCode,
    },
    Approved,
    Gone,
}

/// The joining phone's half-finished request.
#[derive(Clone, Debug)]
pub struct JoinRequest {
    pub confirmation: ConfirmationCode,
    /// Holds the secret, and keeps it out of logs.
    invite: InviteLink,
    /// Chosen when asking, so that polling again after a lost answer is safe.
    device_token: DeviceToken,
}

/// Where a new member stands when it is let in.
pub(super) struct Entry {
    pub server_url: ServerUrl,
    pub family_id: Uuid,
    pub head: Uuid,
    pub device_token: DeviceToken,
    pub name: Name,
    pub agent: bool,
    /// The relay sequence it reads after.
    pub after: i64,
}

fn invite_verifier(secret: &[u8]) -> Digest {
    Digest::from(derive("invite-verifier", &[secret]))
}

impl Device {
    // ---- the owner's side ----------------------------------------------------------

    /// An invitation for a phone.
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
        Ok(InviteTicket {
            invite_id,
            link: InviteLink::new(
                membership.server_url,
                membership.family_id,
                invite_id,
                secret,
            ),
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
                    .context("the relay sent no key package")?;
                InviteProgress::Requested {
                    device_id,
                    confirmation: confirmation_code(ticket.link.secret(), key_package.as_slice()),
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
        let key_package = status
            .pending_key_package
            .context("the relay sent no key package")?
            .into_vec();
        // The joiner must find the whole history in the snapshot.
        self.flush().await?;
        let welcome = self.commit_add(device_id, &key_package).await?;
        api.approve_join(
            membership.family_id,
            &membership.device_token,
            ticket.invite_id,
            &ApproveJoin {
                device_id,
                welcome: Bytes::from(welcome),
            },
        )
        .await?;
        self.send_snapshot().await?;
        self.flush().await
    }

    /// Adds a member: the commit goes to the relay (again, after catching up,
    /// if the group moved on meanwhile); the welcome comes back for the member.
    pub(super) async fn commit_add(
        &mut self,
        device_id: Uuid,
        key_package: &[u8],
    ) -> Result<Vec<u8>> {
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

    /// Step 1: ask the owner to let this phone in.
    pub async fn request_join(&mut self, link: &InviteLink) -> Result<JoinRequest> {
        ensure!(self.membership.is_none(), "already in a family");
        let invite = link.clone();
        let secret = invite.secret().to_vec();
        let key_package = self.mls.key_package()?;
        Api::new(invite.server())?
            .request_join(
                invite.invite(),
                &InviteProof {
                    verifier: invite_verifier(&secret),
                    device_id: self.device_id(),
                    key_package: Bytes::from(key_package.as_slice()),
                },
            )
            .await
            .context("the invitation is expired or already used")?;
        Ok(JoinRequest {
            confirmation: confirmation_code(&secret, &key_package),
            invite,
            device_token: DeviceToken::generate(),
        })
    }

    /// Step 2, polled until the owner approved: `Ok(false)` means not yet.
    pub async fn complete_join(&mut self, request: &JoinRequest, my_name: &str) -> Result<bool> {
        let my_name: Name = my_name.parse()?;
        let api = Api::new(request.invite.server())?;
        let claimed = match api
            .claim_invite(
                request.invite.invite(),
                &ClaimInvite {
                    verifier: invite_verifier(request.invite.secret()),
                    device_id: self.device_id(),
                    token_hash: request.device_token.hash(),
                },
            )
            .await
        {
            Ok(claimed) => claimed,
            Err(error) if HttpError::is_gone(&error) => return Ok(false),
            Err(error) => return Err(error),
        };
        ensure!(
            claimed.family_id == request.invite.family(),
            "the invitation belongs to another family"
        );
        let (family_id, head) = self.mls.join(claimed.welcome.as_slice())?;
        ensure!(
            family_id == claimed.family_id,
            "the welcome is for another family"
        );
        self.enter_family(Entry {
            server_url: api.base().clone(),
            family_id,
            head,
            device_token: request.device_token.clone(),
            name: my_name.clone(),
            agent: false,
            after: claimed.after,
        })
        .await?;
        // Announce ourselves.
        self.run(FamilyCommand::Join { name: my_name }).await?;
        self.flush().await?;
        Ok(true)
    }

    /// The last step of getting in, for a phone and for an AI agent: this
    /// device is a member now; the history comes from the head as a snapshot,
    /// right after the welcome, so wait for it.
    pub(super) async fn enter_family(&mut self, entry: Entry) -> Result<()> {
        self.save_membership(Membership {
            server_url: entry.server_url,
            family_id: entry.family_id,
            head: entry.head,
            device_token: entry.device_token,
            registered: true,
            owner: false,
            name: entry.name,
            agent: entry.agent,
            removed: false,
            restoring: false,
        })?;
        self.events.lock().advance_cursor(entry.after)?;
        let deadline = std::time::Instant::now() + SNAPSHOT_WAIT;
        loop {
            self.pull().await?;
            if self.state()?.family_id.is_some() {
                return Ok(());
            }
            ensure!(
                std::time::Instant::now() < deadline,
                "the head did not send the family's history"
            );
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }
}
