//! What an invitation carries to the other phone: the relay, the family, the
//! invitation, and its secret, written as the link `tackly://join?c=<payload>`
//! (a QR code shows the same). A link is only made from something that reads
//! as one, so whoever holds an [`InviteLink`] holds a whole, valid invitation.

use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::infrastructure::{crypto, server_url::ServerUrl};

const PREFIX: &str = "tackly://join?c=";

#[derive(Clone, PartialEq, Eq)]
pub struct InviteLink {
    server: ServerUrl,
    family: Uuid,
    invite: Uuid,
    secret: Vec<u8>,
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum InviteLinkError {
    #[error("that is not a Tackly invitation link")]
    NotALink,
    #[error("the invitation link is damaged")]
    Damaged,
}

/// How the link's payload is written.
#[derive(Serialize, Deserialize)]
struct Payload {
    server: ServerUrl,
    family: Uuid,
    invite: Uuid,
    /// base64url
    secret: String,
}

impl InviteLink {
    pub fn new(server: ServerUrl, family: Uuid, invite: Uuid, secret: Vec<u8>) -> Self {
        Self {
            server,
            family,
            invite,
            secret,
        }
    }

    pub fn server(&self) -> &ServerUrl {
        &self.server
    }

    pub fn family(&self) -> Uuid {
        self.family
    }

    pub fn invite(&self) -> Uuid {
        self.invite
    }

    /// Never shown or logged: it is what the six digits are made from.
    pub(super) fn secret(&self) -> &[u8] {
        &self.secret
    }
}

/// Read leniently: a bare payload works, and so does a link that a messenger
/// wrapped, padded with whitespace, or followed with more.
impl FromStr for InviteLink {
    type Err = InviteLinkError;

    fn from_str(text: &str) -> Result<Self, InviteLinkError> {
        let compact: String = text.split_whitespace().collect();
        let payload = compact.strip_prefix(PREFIX).unwrap_or(&compact);
        let payload = payload.split(['&', '#']).next().unwrap_or_default();
        if payload.is_empty() {
            return Err(InviteLinkError::NotALink);
        }
        let json = crypto::decode(payload).map_err(|_| InviteLinkError::NotALink)?;
        let payload: Payload =
            serde_json::from_slice(&json).map_err(|_| InviteLinkError::Damaged)?;
        let secret = crypto::decode(&payload.secret).map_err(|_| InviteLinkError::Damaged)?;
        Ok(Self::new(
            payload.server,
            payload.family,
            payload.invite,
            secret,
        ))
    }
}

impl fmt::Display for InviteLink {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let payload = Payload {
            server: self.server.clone(),
            family: self.family,
            invite: self.invite,
            secret: crypto::encode(&self.secret),
        };
        let json = serde_json::to_vec(&payload).map_err(|_| fmt::Error)?;
        write!(f, "{PREFIX}{}", crypto::encode(&json))
    }
}

impl fmt::Debug for InviteLink {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InviteLink")
            .field("server", &self.server)
            .field("family", &self.family)
            .field("invite", &self.invite)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> InviteLink {
        InviteLink::new(
            "http://127.0.0.1:3000".parse().expect("a server"),
            Uuid::from_u128(1),
            Uuid::from_u128(2),
            vec![7; 32],
        )
    }

    #[test]
    fn a_link_reads_back_as_what_it_was_made_from() {
        let link = sample();
        assert_eq!(link.to_string().parse::<InviteLink>(), Ok(link));
    }

    #[test]
    fn links_are_read_leniently() {
        let link = sample();
        let written = link.to_string();
        let payload = written.trim_start_matches(PREFIX).to_owned();
        let (front, back) = payload.split_at(payload.len() / 2);
        for text in [
            written.clone(),
            format!("  {written}\n"),
            format!("{PREFIX}{front}\n  {back}"),
            payload.clone(),
            format!("{written}&utm=x"),
        ] {
            assert_eq!(text.parse::<InviteLink>(), Ok(link.clone()), "{text:?}");
        }
    }

    #[test]
    fn other_text_is_not_a_link() {
        assert_eq!("  ".parse::<InviteLink>(), Err(InviteLinkError::NotALink));
        assert_eq!(
            "hello there".parse::<InviteLink>(),
            Err(InviteLinkError::NotALink)
        );
        // Readable, but not an invitation.
        let damaged = format!("{PREFIX}{}", crypto::encode(b"{}"));
        assert_eq!(damaged.parse::<InviteLink>(), Err(InviteLinkError::Damaged));
    }

    #[test]
    fn the_secret_stays_out_of_logs() {
        assert!(!format!("{:?}", sample()).contains("secret"));
    }
}
