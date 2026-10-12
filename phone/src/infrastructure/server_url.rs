//! The address of a relay, checked once: whatever holds a [`ServerUrl`] holds
//! something that can be called. Typed text becomes one with `parse`.

use std::{fmt, net::SocketAddr, str::FromStr};

use serde::{Deserialize, Serialize};

/// An `http://` or `https://` address with a host, and no query, fragment or
/// login. Spaces around it and a slash at its end are dropped.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ServerUrl(String);

#[derive(Debug, PartialEq, Eq)]
pub enum ServerUrlError {
    /// Not an `http://` or `https://` address.
    Scheme,
    /// No host after the scheme.
    Host,
    /// A query, fragment or login: none of them belongs to a relay's address.
    Extras,
}

impl fmt::Display for ServerUrlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Scheme => "the server address must start with http:// or https://",
            Self::Host => "the server address needs a host, such as https://relay.example.org",
            Self::Extras => "the server address is only the relay's address, nothing after it",
        })
    }
}

impl std::error::Error for ServerUrlError {}

impl ServerUrl {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// A relay listening on `addr` without TLS (the tests' relay).
    pub fn http(addr: SocketAddr) -> Self {
        Self(format!("http://{addr}"))
    }
}

impl FromStr for ServerUrl {
    type Err = ServerUrlError;

    fn from_str(text: &str) -> Result<Self, ServerUrlError> {
        let text = text.trim();
        let url = reqwest::Url::parse(text).map_err(|_| {
            if text.starts_with("http://") || text.starts_with("https://") {
                ServerUrlError::Host
            } else {
                ServerUrlError::Scheme
            }
        })?;
        if !matches!(url.scheme(), "http" | "https") {
            return Err(ServerUrlError::Scheme);
        }
        if url.host_str().is_none_or(str::is_empty) {
            return Err(ServerUrlError::Host);
        }
        if url.query().is_some()
            || url.fragment().is_some()
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err(ServerUrlError::Extras);
        }
        Ok(Self(url.as_str().trim_end_matches('/').to_owned()))
    }
}

impl TryFrom<String> for ServerUrl {
    type Error = ServerUrlError;

    fn try_from(text: String) -> Result<Self, ServerUrlError> {
        text.parse()
    }
}

impl From<ServerUrl> for String {
    fn from(url: ServerUrl) -> Self {
        url.0
    }
}

impl fmt::Display for ServerUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_are_tidied_and_checked() {
        for (typed, kept) in [
            ("http://127.0.0.1:3000", "http://127.0.0.1:3000"),
            (
                "  https://relay.example.org/  ",
                "https://relay.example.org",
            ),
            ("https://example.org/tackly/", "https://example.org/tackly"),
            ("HTTPS://Relay.Example.org", "https://relay.example.org"),
        ] {
            assert_eq!(
                typed.parse::<ServerUrl>().map(String::from).as_deref(),
                Ok(kept)
            );
        }
        assert_eq!(
            "relay.example.org".parse::<ServerUrl>(),
            Err(ServerUrlError::Scheme)
        );
        assert_eq!(
            "ftp://relay.example.org".parse::<ServerUrl>(),
            Err(ServerUrlError::Scheme)
        );
        assert_eq!("".parse::<ServerUrl>(), Err(ServerUrlError::Scheme));
        assert_eq!("https://".parse::<ServerUrl>(), Err(ServerUrlError::Host));
        for extras in [
            "https://relay.example.org/?a=1",
            "https://relay.example.org/#top",
            "https://me:secret@relay.example.org",
        ] {
            assert_eq!(
                extras.parse::<ServerUrl>(),
                Err(ServerUrlError::Extras),
                "{extras}"
            );
        }
    }

    #[test]
    fn a_stored_address_is_checked_when_it_is_read_back() {
        let url: ServerUrl = serde_json::from_str("\"http://127.0.0.1:3000\"").expect("valid");
        assert_eq!(
            serde_json::to_string(&url).expect("json"),
            "\"http://127.0.0.1:3000\""
        );
        assert!(serde_json::from_str::<ServerUrl>("\"not a url\"").is_err());
    }
}
