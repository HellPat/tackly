use aes_gcm::{
    Aes256Gcm, KeyInit, Nonce,
    aead::{AeadInPlace, generic_array::GenericArray},
};
use anyhow::{Context, Result, ensure};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::{RngCore, rngs::OsRng};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SealedData {
    pub nonce: String,
    pub ciphertext: String,
    pub mac: String,
}

pub fn random_bytes<const N: usize>() -> [u8; N] {
    let mut bytes = [0; N];
    OsRng.fill_bytes(&mut bytes);
    bytes
}

pub fn encode(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

pub fn decode(value: &str) -> Result<Vec<u8>> {
    URL_SAFE_NO_PAD.decode(value).context("decode base64url")
}

pub fn seal(key: &[u8], plaintext: &[u8], aad: &str) -> Result<SealedData> {
    ensure!(key.len() == 32, "AES-256 key must be 32 bytes");
    let cipher = Aes256Gcm::new_from_slice(key).expect("checked length");
    let nonce = random_bytes::<12>();
    let mut body = plaintext.to_vec();
    let mac = cipher
        .encrypt_in_place_detached(Nonce::from_slice(&nonce), aad.as_bytes(), &mut body)
        .map_err(|_| anyhow::anyhow!("encrypt"))?;
    Ok(SealedData {
        nonce: encode(&nonce),
        ciphertext: encode(&body),
        mac: encode(&mac),
    })
}

pub fn open(key: &[u8], sealed: &SealedData, aad: &str) -> Result<Vec<u8>> {
    ensure!(key.len() == 32, "AES-256 key must be 32 bytes");
    let nonce = decode(&sealed.nonce)?;
    let mut body = decode(&sealed.ciphertext)?;
    let mac = decode(&sealed.mac)?;
    ensure!(
        nonce.len() == 12 && mac.len() == 16,
        "invalid AES-GCM envelope"
    );
    let cipher = Aes256Gcm::new_from_slice(key).expect("checked length");
    cipher
        .decrypt_in_place_detached(
            Nonce::from_slice(&nonce),
            aad.as_bytes(),
            &mut body,
            GenericArray::from_slice(&mac),
        )
        .map_err(|_| anyhow::anyhow!("ciphertext authentication failed"))?;
    Ok(body)
}

/// Domain-separated SHA-256, used for verifiers and derived keys.
pub fn derive(label: &str, parts: &[&[u8]]) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut hash = Sha256::new();
    hash.update(label.as_bytes());
    hash.update([0]);
    for part in parts {
        hash.update(part);
    }
    hash.finalize().into()
}
