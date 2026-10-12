//! The phone's place in a family, kept with the other secrets.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tackly_domain::Name;
use uuid::Uuid;

use crate::infrastructure::{device_token::DeviceToken, secrets::Secrets, server_url::ServerUrl};

const SECRET_NAME: &str = "membership";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Membership {
    pub server_url: ServerUrl,
    pub family_id: Uuid,
    /// The head of the family: the only one whose commits (adding and
    /// removing members) and history snapshots this phone accepts.
    pub head: Uuid,
    /// This phone's secret for the relay, chosen here; the relay keeps its hash.
    pub device_token: DeviceToken,
    /// False while the family exists only on this phone (created offline).
    pub registered: bool,
    pub owner: bool,
    pub name: Name,
    /// An AI agent: reads along, sends nothing.
    #[serde(default)]
    pub agent: bool,
    /// The head took this phone out of the family.
    #[serde(default)]
    pub removed: bool,
    /// The head's phone came back from a backup and takes the history from
    /// any member, until the first snapshot is in.
    #[serde(default)]
    pub restoring: bool,
}

impl Membership {
    pub(super) fn load(secrets: &Secrets) -> Result<Option<Self>> {
        secrets
            .read(SECRET_NAME)?
            .map(|bytes| serde_json::from_slice(&bytes))
            .transpose()
            .context("read family membership")
    }

    pub(super) fn save(&self, secrets: &Secrets) -> Result<()> {
        secrets.write(SECRET_NAME, &serde_json::to_vec(self)?)
    }

    pub(super) fn forget(secrets: &Secrets) -> Result<()> {
        secrets.delete(SECRET_NAME)
    }
}
