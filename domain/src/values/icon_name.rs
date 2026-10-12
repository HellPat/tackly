use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};

/// The longest icon name there is (Material Symbols names are far shorter).
const MAX_LEN: usize = 40;

/// The name of an icon in the app's icon font (Material Symbols): lower case
/// letters, digits and underscores, such as `sailing` or `sentiment_satisfied`.
/// What another phone sends as an icon is drawn as text in that font, so it is
/// kept to names that can only be icons.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct IconName(String);

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
#[error("an icon name is lower case letters, digits and underscores")]
pub struct NotAnIconName;

impl IconName {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for IconName {
    type Err = NotAnIconName;

    fn from_str(text: &str) -> Result<Self, NotAnIconName> {
        let fits = !text.is_empty()
            && text.len() <= MAX_LEN
            && text
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
        if fits {
            Ok(Self(text.to_owned()))
        } else {
            Err(NotAnIconName)
        }
    }
}

impl TryFrom<String> for IconName {
    type Error = NotAnIconName;

    fn try_from(text: String) -> Result<Self, NotAnIconName> {
        text.parse()
    }
}

impl From<IconName> for String {
    fn from(icon: IconName) -> Self {
        icon.0
    }
}

impl fmt::Display for IconName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_names_are_snake_case_words() {
        for name in ["sailing", "sentiment_satisfied", "looks_3"] {
            assert!(name.parse::<IconName>().is_ok(), "{name}");
        }
    }

    #[test]
    fn anything_else_is_not() {
        let too_long = "a".repeat(MAX_LEN + 1);
        for name in ["", "Sailing", "two words", "<b>", "a/b", too_long.as_str()] {
            assert_eq!(name.parse::<IconName>(), Err(NotAnIconName), "{name:?}");
        }
    }
}
