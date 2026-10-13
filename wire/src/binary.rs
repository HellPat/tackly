//! Binary values on the wire. Both travel as unpadded base64url text in JSON;
//! reading one that is not valid base64url (or, for a [`Digest`], not exactly
//! 32 bytes) is an error where it is deserialized, so a handler never sees a
//! bad one.

use std::{fmt, str::FromStr};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest as _, Sha256};

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum BinaryError {
    #[error("not base64url text")]
    NotBase64,
    #[error("a digest is exactly 32 bytes")]
    NotADigest,
}

/// Bytes of any length: an MLS message, a key package, a welcome, a backup.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct Bytes(Vec<u8>);

impl Bytes {
    pub fn as_slice(&self) -> &[u8] {
        &self.0
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn into_vec(self) -> Vec<u8> {
        self.0
    }
}

impl From<Vec<u8>> for Bytes {
    fn from(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }
}

impl From<&[u8]> for Bytes {
    fn from(bytes: &[u8]) -> Self {
        Self(bytes.to_vec())
    }
}

impl FromStr for Bytes {
    type Err = BinaryError;

    fn from_str(text: &str) -> Result<Self, BinaryError> {
        URL_SAFE_NO_PAD
            .decode(text)
            .map(Self)
            .map_err(|_| BinaryError::NotBase64)
    }
}

impl fmt::Display for Bytes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&URL_SAFE_NO_PAD.encode(&self.0))
    }
}

/// What the bytes are is not logged, only how many there are.
impl fmt::Debug for Bytes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Bytes({})", self.0.len())
    }
}

impl Serialize for Bytes {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Bytes {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

/// Exactly 32 bytes: a SHA-256 hash, such as what the relay keeps of a device
/// token, or the verifier of an invitation.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct Digest([u8; 32]);

impl Digest {
    /// The SHA-256 of `bytes`.
    pub fn sha256(bytes: &[u8]) -> Self {
        Self(Sha256::digest(bytes).into())
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl From<[u8; 32]> for Digest {
    fn from(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
}

impl FromStr for Digest {
    type Err = BinaryError;

    fn from_str(text: &str) -> Result<Self, BinaryError> {
        let bytes: Bytes = text.parse()?;
        <[u8; 32]>::try_from(bytes.as_slice())
            .map(Self)
            .map_err(|_| BinaryError::NotADigest)
    }
}

impl fmt::Display for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&URL_SAFE_NO_PAD.encode(self.0))
    }
}

impl fmt::Debug for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Digest({self})")
    }
}

impl Serialize for Digest {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Digest {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_travel_as_base64url_text() {
        let bytes = Bytes::from(vec![0xfb, 0xff, 0x00]);
        assert_eq!(bytes.to_string(), "-_8A");
        assert_eq!("-_8A".parse::<Bytes>(), Ok(bytes.clone()));
        assert_eq!(serde_json::to_string(&bytes).expect("json"), "\"-_8A\"");
        assert!("not base64!".parse::<Bytes>().is_err());
        assert!(
            serde_json::from_str::<Bytes>("\"+/8A\"").is_err(),
            "standard base64 is not base64url"
        );
    }

    #[test]
    fn a_digest_is_32_bytes_exactly() {
        let digest = Digest::sha256(b"token");
        assert_eq!(digest.to_string().parse::<Digest>(), Ok(digest));
        let short = Bytes::from(vec![1; 31]).to_string();
        let long = Bytes::from(vec![1; 33]).to_string();
        assert_eq!(short.parse::<Digest>(), Err(BinaryError::NotADigest));
        assert_eq!(long.parse::<Digest>(), Err(BinaryError::NotADigest));
        assert_eq!("???".parse::<Digest>(), Err(BinaryError::NotBase64));
    }

    #[test]
    fn what_is_inside_stays_out_of_logs() {
        assert_eq!(format!("{:?}", Bytes::from(vec![1, 2, 3])), "Bytes(3)");
    }

    #[test]
    fn bytes_know_their_length_and_give_up_their_contents() {
        let bytes = Bytes::from([1, 2, 3].as_slice());
        assert_eq!((bytes.len(), bytes.is_empty()), (3, false));
        assert!(Bytes::from(Vec::new()).is_empty());
        assert_eq!(bytes.into_vec(), vec![1, 2, 3]);
    }

    #[test]
    fn a_digest_is_its_32_bytes_and_shows_them() {
        let digest = Digest::from([7; 32]);
        assert_eq!(digest.as_bytes(), &[7; 32]);
        assert_eq!(format!("{digest:?}"), format!("Digest({digest})"));
        let json = serde_json::to_string(&digest).expect("json");
        assert_eq!(json, format!("\"{digest}\""));
        assert_eq!(serde_json::from_str::<Digest>(&json).ok(), Some(digest));
        assert!(serde_json::from_str::<Digest>("1").is_err());
        assert!(serde_json::from_str::<Bytes>("1").is_err());
    }
}
