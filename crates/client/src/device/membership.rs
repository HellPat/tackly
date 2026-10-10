//! The phone's place in a family, kept with the other secrets.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::secrets::Secrets;

const SECRET_NAME: &str = "membership";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Membership {
    pub server_url: String,
    pub family_id: Uuid,
    /// Base64url of the key that encrypts the family's events.
    pub family_key: String,
    /// This phone's secret for the relay, chosen here; the relay keeps its hash.
    pub device_token: String,
    /// False while the family exists only on this phone (created offline).
    pub registered: bool,
    pub owner: bool,
    pub name: String,
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
