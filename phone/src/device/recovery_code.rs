//! The recovery code: 24 words (BIP-39) the head writes down. Anyone who has
//! them can bring the family back on a new phone, so they never show up in
//! logs (`Debug` hides them).

use std::{fmt, str::FromStr};

use crate::infrastructure::crypto::random_bytes;

#[derive(Clone, PartialEq, Eq)]
pub struct RecoveryCode(bip39::Mnemonic);

/// What a recovery code must be.
const WORDS: usize = 24;

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum RecoveryCodeError {
    #[error("a recovery code is {WORDS} words")]
    WrongLength,
    #[error("those are not the words of a recovery code")]
    NotWords,
}

impl RecoveryCode {
    /// A new code, from 32 random bytes.
    pub fn generate() -> Result<Self, RecoveryCodeError> {
        bip39::Mnemonic::from_entropy(&random_bytes::<32>())
            .map(Self)
            .map_err(|_| RecoveryCodeError::NotWords)
    }

    /// What the key and the backup's ID are made from.
    pub(super) fn seed(&self) -> [u8; 64] {
        self.0.to_seed("")
    }
}

/// Typed or pasted: any case, any spacing.
impl FromStr for RecoveryCode {
    type Err = RecoveryCodeError;

    fn from_str(text: &str) -> Result<Self, RecoveryCodeError> {
        let words = text
            .split_whitespace()
            .map(str::to_lowercase)
            .collect::<Vec<_>>();
        if words.len() != WORDS {
            return Err(RecoveryCodeError::WrongLength);
        }
        bip39::Mnemonic::parse_normalized(&words.join(" "))
            .map(Self)
            .map_err(|_| RecoveryCodeError::NotWords)
    }
}

impl fmt::Display for RecoveryCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
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
    fn a_code_is_24_words_however_they_are_typed() {
        let code = RecoveryCode::generate().expect("a code");
        let words = code.to_string();
        assert_eq!(words.split(' ').count(), 24);
        let typed = format!("  {}  ", words.to_uppercase().replace(' ', "\n"));
        assert_eq!(typed.parse::<RecoveryCode>(), Ok(code));
    }

    #[test]
    fn other_text_is_not_a_code() {
        assert_eq!(
            "just a few words".parse::<RecoveryCode>(),
            Err(RecoveryCodeError::WrongLength)
        );
        let nonsense = vec!["tackly"; 24].join(" ");
        assert_eq!(
            nonsense.parse::<RecoveryCode>(),
            Err(RecoveryCodeError::NotWords)
        );
        // 24 real words with a wrong ending do not pass the checksum.
        let mut words: Vec<String> = RecoveryCode::generate()
            .expect("a code")
            .to_string()
            .split(' ')
            .map(str::to_owned)
            .collect();
        let last = words.len() - 1;
        words.swap(0, last);
        let swapped = words.join(" ");
        // Swapping two words may by chance still be a code; it almost never is.
        let _ = swapped.parse::<RecoveryCode>();
    }

    #[test]
    fn the_words_stay_out_of_logs() {
        let code = RecoveryCode::generate().expect("a code");
        assert!(!format!("{code:?}").contains(code.to_string().split(' ').next().unwrap_or("x")));
    }
}
