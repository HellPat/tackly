use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};

/// A name or title a person typed: text that is not empty once the spaces
/// around it are gone (they are dropped).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Name(String);

/// There was nothing but spaces.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
#[error("names and titles must not be empty")]
pub struct EmptyName;

impl Name {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for Name {
    type Err = EmptyName;

    fn from_str(text: &str) -> Result<Self, EmptyName> {
        let text = text.trim();
        if text.is_empty() {
            Err(EmptyName)
        } else {
            Ok(Self(text.to_owned()))
        }
    }
}

impl TryFrom<String> for Name {
    type Error = EmptyName;

    fn try_from(text: String) -> Result<Self, EmptyName> {
        text.parse()
    }
}

impl From<Name> for String {
    fn from(name: Name) -> Self {
        name.0
    }
}

impl fmt::Display for Name {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spaces_around_a_name_are_dropped() {
        assert_eq!(
            " Milk ".parse::<Name>().map(String::from).as_deref(),
            Ok("Milk")
        );
        let name: Name = " Milk ".parse().expect("a name");
        assert_eq!((name.as_str(), name.to_string().as_str()), ("Milk", "Milk"));
    }

    #[test]
    fn nothing_is_not_a_name() {
        for text in ["", "   ", "\n\t"] {
            assert_eq!(text.parse::<Name>(), Err(EmptyName), "{text:?}");
        }
    }

    #[test]
    fn a_name_read_back_from_an_event_is_checked_too() {
        assert!(serde_json::from_str::<Name>("\"Milk\"").is_ok());
        assert!(serde_json::from_str::<Name>("\"  \"").is_err());
    }
}
