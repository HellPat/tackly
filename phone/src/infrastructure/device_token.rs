//! A device's secret for the relay. The phone makes it (32 random bytes as
//! base64url text), keeps it, and tells the relay only its hash. It never shows
//! up in logs.

use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};
use tackly_wire::{BinaryError, Bytes, Digest};

use super::crypto::random_bytes;

const BYTES: usize = 32;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct DeviceToken(String);

impl DeviceToken {
    pub fn generate() -> Self {
        Self(Bytes::from(random_bytes::<BYTES>().as_slice()).to_string())
    }

    /// What the relay keeps of it.
    pub fn hash(&self) -> Digest {
        tackly_wire::token_hash(&self.0)
    }

    /// For the `Authorization` header.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for DeviceToken {
    type Err = BinaryError;

    fn from_str(text: &str) -> Result<Self, BinaryError> {
        let bytes: Bytes = text.parse()?;
        if bytes.len() == BYTES {
            Ok(Self(text.to_owned()))
        } else {
            Err(BinaryError::NotADigest)
        }
    }
}

impl TryFrom<String> for DeviceToken {
    type Error = BinaryError;

    fn try_from(text: String) -> Result<Self, BinaryError> {
        text.parse()
    }
}

impl From<DeviceToken> for String {
    fn from(token: DeviceToken) -> Self {
        token.0
    }
}

impl fmt::Debug for DeviceToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("DeviceToken(…)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_token_is_32_random_bytes_and_hashes_to_what_the_relay_keeps() {
        let token = DeviceToken::generate();
        assert_ne!(token, DeviceToken::generate());
        assert_eq!(token.as_str().parse::<DeviceToken>(), Ok(token.clone()));
        assert_eq!(token.hash(), tackly_wire::token_hash(token.as_str()));
    }

    #[test]
    fn a_short_one_is_not_a_token() {
        assert_eq!("AAAA".parse::<DeviceToken>(), Err(BinaryError::NotADigest));
    }

    #[test]
    fn other_text_is_not_a_token() {
        assert!("short".parse::<DeviceToken>().is_err());
        assert!("???".parse::<DeviceToken>().is_err());
    }

    #[test]
    fn it_stays_out_of_logs() {
        let token = DeviceToken::generate();
        assert!(!format!("{token:?}").contains(token.as_str()));
    }
}
