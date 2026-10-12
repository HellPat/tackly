//! The recovery code: what the head keeps safe (in a password manager, say) to
//! bring the family back on a new phone. 160 random bits written as 32
//! characters, and three more that catch a typo, in seven groups of five:
//! `K7QMT-4ZP9X-WDHC2-RJ8V5-BN3GA-6EYFS-T4K2M`. The characters are Crockford's
//! base32 (no I, L, O or U), so a read-aloud or hand-copied code survives
//! confusions, case and dashes. Anyone who has the code can open the family's
//! backup, so it never shows up in logs (`Debug` hides it).

use std::{fmt, str::FromStr};

use sha2::{Digest, Sha256};

use crate::infrastructure::crypto::random_bytes;

/// 160 bits: more than anyone can try.
const SECRET_BYTES: usize = 20;
/// 32 characters carry the secret.
const SECRET_CHARS: usize = 32;
/// Three more are a checksum: 15 bits, so a typo is caught 32,767 times in 32,768.
const CHECK_CHARS: usize = 3;
const GROUP: usize = 5;
const ALPHABET: base32::Alphabet = base32::Alphabet::Crockford;

#[derive(Clone, PartialEq, Eq)]
pub struct RecoveryCode([u8; SECRET_BYTES]);

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum RecoveryCodeError {
    #[error("that is not a recovery code")]
    NotACode,
    #[error("that recovery code has a typo: check it against the saved one")]
    Typo,
}

impl RecoveryCode {
    pub fn generate() -> Self {
        Self(random_bytes::<SECRET_BYTES>())
    }

    /// What the key and the backup's ID are made from.
    pub(super) fn secret(&self) -> &[u8; SECRET_BYTES] {
        &self.0
    }

    /// The characters that catch a typo, from the secret itself.
    fn check(secret: &[u8]) -> String {
        let hash = Sha256::digest([b"tackly-recovery-check".as_slice(), secret].concat());
        // Two bytes make four characters; the first three are the first 15 bits.
        base32::encode(ALPHABET, &hash[..2])[..CHECK_CHARS].to_owned()
    }
}

/// Typed, pasted or read aloud: any case, with or without dashes and spaces.
impl FromStr for RecoveryCode {
    type Err = RecoveryCodeError;

    fn from_str(text: &str) -> Result<Self, RecoveryCodeError> {
        let characters: String = text
            .chars()
            .filter(|c| !c.is_whitespace() && *c != '-')
            .flat_map(char::to_uppercase)
            .collect();
        if characters.len() != SECRET_CHARS + CHECK_CHARS || !characters.is_ascii() {
            return Err(RecoveryCodeError::NotACode);
        }
        let (data, check) = characters.split_at(SECRET_CHARS);
        let secret: [u8; SECRET_BYTES] = base32::decode(ALPHABET, data)
            .and_then(|bytes| bytes.try_into().ok())
            .ok_or(RecoveryCodeError::NotACode)?;
        // The checksum is compared as the characters it is written in, with
        // the same confusions allowed as in the rest.
        let expected = Self::check(&secret);
        let written = base32::decode(ALPHABET, &format!("{check}0"))
            .map(|bytes| base32::encode(ALPHABET, &bytes)[..CHECK_CHARS].to_owned());
        if written.as_deref() == Some(expected.as_str()) {
            Ok(Self(secret))
        } else {
            Err(RecoveryCodeError::Typo)
        }
    }
}

impl fmt::Display for RecoveryCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let characters = format!(
            "{}{}",
            base32::encode(ALPHABET, &self.0),
            Self::check(&self.0)
        );
        for (i, character) in characters.chars().enumerate() {
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
    fn a_code_is_seven_groups_of_five() {
        let shown = RecoveryCode::generate().to_string();
        assert_eq!(shown.len(), 7 * GROUP + 6);
        assert!(shown.split('-').all(|group| group.len() == GROUP));
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
        ] {
            assert_eq!(typed.parse::<RecoveryCode>(), Ok(code.clone()), "{typed:?}");
        }
    }

    #[test]
    fn look_alike_characters_are_forgiven() {
        // Crockford: O is 0, I and L are 1.
        let code = RecoveryCode::generate();
        let confused = code.to_string().replace('0', "O").replace('1', "l");
        assert_eq!(confused.parse::<RecoveryCode>(), Ok(code));
    }

    #[test]
    fn two_codes_differ() {
        assert_ne!(RecoveryCode::generate(), RecoveryCode::generate());
    }

    #[test]
    fn a_typo_is_caught() {
        let shown = RecoveryCode::generate().to_string();
        // Change one character to another one that is allowed.
        let mut typed: Vec<char> = shown.chars().collect();
        typed[3] = if typed[3] == '2' { '3' } else { '2' };
        let typed: String = typed.into_iter().collect();
        assert_eq!(typed.parse::<RecoveryCode>(), Err(RecoveryCodeError::Typo));
    }

    #[test]
    fn other_text_is_not_a_code() {
        for text in [
            "",
            "just a few words",
            "K7QMT-4ZP9X",
            "!!!!!-!!!!!-!!!!!-!!!!!-!!!!!-!!!!!-!!!!!",
        ] {
            assert_eq!(
                text.parse::<RecoveryCode>(),
                Err(RecoveryCodeError::NotACode),
                "{text:?}"
            );
        }
    }

    #[test]
    fn the_code_stays_out_of_logs() {
        let code = RecoveryCode::generate();
        let first_group = code.to_string()[..GROUP].to_owned();
        assert!(!format!("{code:?}").contains(&first_group));
    }
}
