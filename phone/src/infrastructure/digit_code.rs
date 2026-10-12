//! Digits two people compare to see that they talk about the same thing: the
//! head and a phone that asks to join, the head and an AI agent that asks for
//! access. Both sides derive the digits from what they hold; a relay in the
//! middle cannot make them agree.

use std::{fmt, str::FromStr};

/// `LENGTH` decimal digits, shown in groups of `GROUP`. Codes of different
/// kinds are different types, so one is never compared with another.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DigitCode<const LENGTH: usize, const GROUP: usize>(String);

/// Six digits, for a phone that asks to join with an invitation: made from the
/// invitation's secret and the phone's key package.
pub type ConfirmationCode = DigitCode<6, 6>;

/// Twelve digits, in groups of four, for an AI agent that asks for access:
/// made from the agent's key package alone (there is no secret to bind them
/// to, so they are longer).
pub type AgentCode = DigitCode<12, 4>;

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
#[error("that is not a code of the right number of digits")]
pub struct NotACode;

impl<const LENGTH: usize, const GROUP: usize> DigitCode<LENGTH, GROUP> {
    /// The digits a hash gives (the first eight bytes, as a number).
    pub fn from_hash(hash: &[u8; 32]) -> Self {
        let number = u64::from_be_bytes([
            hash[0], hash[1], hash[2], hash[3], hash[4], hash[5], hash[6], hash[7],
        ]);
        // `LENGTH` is at most 19: ten to that power fits in a u64.
        let modulus = 10_u64.saturating_pow(LENGTH as u32);
        Self(format!("{:0width$}", number % modulus, width = LENGTH))
    }
}

/// Typed with or without the spaces.
impl<const LENGTH: usize, const GROUP: usize> FromStr for DigitCode<LENGTH, GROUP> {
    type Err = NotACode;

    fn from_str(text: &str) -> Result<Self, NotACode> {
        let digits: String = text.split_whitespace().collect();
        if digits.len() == LENGTH && digits.bytes().all(|b| b.is_ascii_digit()) {
            Ok(Self(digits))
        } else {
            Err(NotACode)
        }
    }
}

impl<const LENGTH: usize, const GROUP: usize> fmt::Display for DigitCode<LENGTH, GROUP> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, digit) in self.0.chars().enumerate() {
            if i > 0 && i % GROUP == 0 {
                f.write_str(" ")?;
            }
            write!(f, "{digit}")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hash_gives_the_digits_in_groups() {
        let hash = [7_u8; 32];
        let agent = AgentCode::from_hash(&hash);
        let shown = agent.to_string();
        assert_eq!(shown.len(), 14);
        assert_eq!(
            shown.split(' ').map(str::len).collect::<Vec<_>>(),
            [4, 4, 4]
        );
        assert_eq!(ConfirmationCode::from_hash(&hash).to_string().len(), 6);
    }

    #[test]
    fn a_code_read_back_is_the_same_code_with_or_without_spaces() {
        let code = AgentCode::from_hash(&[1; 32]);
        assert_eq!(code.to_string().parse::<AgentCode>(), Ok(code.clone()));
        assert_eq!(
            code.to_string().replace(' ', "").parse::<AgentCode>(),
            Ok(code)
        );
    }

    #[test]
    fn other_text_is_not_a_code() {
        for text in ["", "12345", "1234567", "12345a", "1234 5678 90"] {
            assert_eq!(text.parse::<ConfirmationCode>(), Err(NotACode), "{text:?}");
        }
        assert_eq!(
            "123456".parse::<ConfirmationCode>().map(|c| c.to_string()),
            Ok("123456".into())
        );
        assert_eq!(
            "1234 5678 9012".parse::<AgentCode>().map(|c| c.to_string()),
            Ok("1234 5678 9012".into())
        );
    }

    #[test]
    fn leading_zeros_are_kept() {
        let mut hash = [0_u8; 32];
        hash[7] = 5;
        assert_eq!(ConfirmationCode::from_hash(&hash).to_string(), "000005");
    }
}
