//! The recovery code: what the head keeps safe (in a password manager, say) to
//! bring the family back on a new phone. It is an age identity (the `age` file
//! encryption format, by the `age` crate): a 256-bit X25519 key from the
//! system's secure random generator, written in Bech32 with a checksum that
//! catches typos. The phone seals its backup to the code's public key; only the
//! code opens it. A backup is a standard age file, so any age tool opens it with
//! the code written in full (`AGE-SECRET-KEY-1…`). Nothing here is homemade
//! cryptography: generating, writing, reading, sealing and opening are all the
//! `age` crate's.
//!
//! The code is shown without the constant `AGE-SECRET-KEY-1` in front, in
//! groups of six, like `QQGZ4J-7XH4NC-…`. It never shows up in logs (`Debug`
//! hides it).

use std::{fmt, str::FromStr, sync::Arc};

use age::{secrecy::ExposeSecret, x25519};
use tackly_wire::Digest;

/// What age writes in front of every secret key.
const PREFIX: &str = "AGE-SECRET-KEY-1";
/// Characters after it: 52 for the 256-bit key, 6 for the checksum.
const BODY: usize = 58;
const GROUP: usize = 6;

#[derive(Clone)]
pub struct RecoveryCode(Arc<x25519::Identity>);

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum RecoveryCodeError {
    #[error("that is not a recovery code")]
    NotACode,
    #[error("that recovery code has a typo: check it against the saved one")]
    Typo,
}

impl RecoveryCode {
    pub fn generate() -> Self {
        Self(Arc::new(x25519::Identity::generate()))
    }

    /// The public half: what a backup is sealed to. The phone keeps it after
    /// it forgot the code itself.
    pub fn recipient(&self) -> x25519::Recipient {
        self.0.to_public()
    }

    /// Opens a backup sealed to this code's recipient.
    pub fn open(&self, backup: &[u8]) -> Result<Vec<u8>, age::DecryptError> {
        age::decrypt(&*self.0, backup)
    }

    /// Where the relay keeps this code's backup.
    pub fn backup_id(&self) -> Digest {
        backup_id(&self.recipient())
    }

    /// The code as age writes it, with its prefix.
    fn written(&self) -> String {
        self.0.to_string().expose_secret().to_owned()
    }
}

/// Seals a backup so that only the code that goes with `recipient` opens it.
pub fn seal(recipient: &x25519::Recipient, plaintext: &[u8]) -> Result<Vec<u8>, age::EncryptError> {
    age::encrypt(recipient, plaintext)
}

/// Where the relay keeps the backup sealed to `recipient`: a hash of the
/// public key, which tells the relay nothing it can use.
pub fn backup_id(recipient: &x25519::Recipient) -> Digest {
    Digest::sha256(format!("tackly-recovery-backup\0{recipient}").as_bytes())
}

impl PartialEq for RecoveryCode {
    fn eq(&self, other: &Self) -> bool {
        self.recipient() == other.recipient()
    }
}

impl Eq for RecoveryCode {}

/// Typed, pasted or read aloud: any case, with or without dashes and spaces,
/// with or without the `AGE-SECRET-KEY-1` in front.
impl FromStr for RecoveryCode {
    type Err = RecoveryCodeError;

    fn from_str(text: &str) -> Result<Self, RecoveryCodeError> {
        let characters: String = text
            .chars()
            .filter(|c| !c.is_whitespace() && *c != '-')
            .flat_map(char::to_uppercase)
            .collect();
        let prefix: String = PREFIX.chars().filter(|c| *c != '-').collect();
        let body = characters.strip_prefix(&prefix).unwrap_or(&characters);
        if body.len() != BODY || !body.is_ascii() {
            return Err(RecoveryCodeError::NotACode);
        }
        format!("{PREFIX}{body}")
            .parse::<x25519::Identity>()
            .map(|identity| Self(Arc::new(identity)))
            .map_err(|_| RecoveryCodeError::Typo)
    }
}

impl fmt::Display for RecoveryCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let written = self.written();
        let body = written.strip_prefix(PREFIX).unwrap_or(&written);
        for (i, character) in body.chars().enumerate() {
            if i > 0 && i % GROUP == 0 {
                f.write_str("-")?;
            }
            write!(f, "{character}")?;
        }
        Ok(())
    }
}

impl fmt::Debug for RecoveryCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("RecoveryCode(…)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_code_is_written_in_groups_of_six() {
        let shown = RecoveryCode::generate().to_string();
        let groups: Vec<&str> = shown.split('-').collect();
        assert_eq!(groups.iter().map(|g| g.len()).sum::<usize>(), BODY);
        assert!(groups.iter().all(|group| group.len() <= GROUP));
        assert!(
            shown
                .chars()
                .all(|c| c == '-' || c.is_ascii_uppercase() || c.is_ascii_digit()),
            "{shown}"
        );
    }

    #[test]
    fn a_code_reads_back_however_it_is_typed() {
        let code = RecoveryCode::generate();
        let shown = code.to_string();
        for typed in [
            shown.clone(),
            shown.to_lowercase(),
            shown.replace('-', ""),
            shown.replace('-', " "),
            format!("  {shown}\n"),
            format!("{PREFIX}{}", shown.replace('-', "")),
            code.written(),
        ] {
            assert_eq!(typed.parse::<RecoveryCode>(), Ok(code.clone()), "{typed:?}");
        }
    }

    #[test]
    fn two_codes_differ() {
        assert_ne!(RecoveryCode::generate(), RecoveryCode::generate());
    }

    #[test]
    fn a_typo_is_caught() {
        let shown = RecoveryCode::generate().to_string();
        let mut typed: Vec<char> = shown.chars().collect();
        typed[3] = if typed[3] == 'Q' { 'P' } else { 'Q' };
        let typed: String = typed.into_iter().collect();
        assert_eq!(typed.parse::<RecoveryCode>(), Err(RecoveryCodeError::Typo));
    }

    #[test]
    fn other_text_is_not_a_code() {
        for text in ["", "just a few words", "QQGZ4J-7XH4NC"] {
            assert_eq!(
                text.parse::<RecoveryCode>(),
                Err(RecoveryCodeError::NotACode),
                "{text:?}"
            );
        }
    }

    #[test]
    fn a_backup_opens_only_with_its_code() {
        let code = RecoveryCode::generate();
        let sealed = seal(&code.recipient(), b"the family").expect("seal");
        assert_eq!(code.open(&sealed).expect("open"), b"the family");
        let typed: RecoveryCode = code.to_string().parse().expect("the same code");
        assert_eq!(typed.open(&sealed).expect("open"), b"the family");
        assert!(RecoveryCode::generate().open(&sealed).is_err());
    }

    #[test]
    fn the_backup_has_an_id_that_follows_the_code() {
        let code = RecoveryCode::generate();
        let typed: RecoveryCode = code.to_string().parse().expect("the same code");
        assert_eq!(code.backup_id(), typed.backup_id());
        assert_ne!(code.backup_id(), RecoveryCode::generate().backup_id());
    }

    #[test]
    fn the_code_stays_out_of_logs() {
        let code = RecoveryCode::generate();
        let first_group = code.to_string()[..GROUP].to_owned();
        assert!(!format!("{code:?}").contains(&first_group));
    }
}
