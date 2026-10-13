//! The address of a relay, checked once: whatever holds a [`ServerUrl`] holds
//! something that can be called. Typed text becomes one with `parse`.

use std::{fmt, net::SocketAddr, str::FromStr};

use serde::{Deserialize, Serialize};

/// An `https://` address with a host, and no query, fragment or login. Plain
/// `http://` is allowed only for a relay on the local network (localhost, a
/// private address, a name without a dot or ending in `.local`): the device
/// token travels as a bearer token, and on the internet it must not be read
/// along the way. Spaces around the address and a slash at its end are dropped.
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
    /// `http://` to a host on the internet.
    Cleartext,
}

impl fmt::Display for ServerUrlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Scheme => "the server address must start with http:// or https://",
            Self::Host => "the server address needs a host, such as https://relay.example.org",
            Self::Extras => "the server address is only the relay's address, nothing after it",
            Self::Cleartext => {
                "a server on the internet needs https://; http:// is only for your own network"
            }
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
        // http and https addresses always have a host once they parse.
        if !matches!(url.scheme(), "http" | "https") {
            return Err(ServerUrlError::Scheme);
        }
        if url.query().is_some()
            || url.fragment().is_some()
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err(ServerUrlError::Extras);
        }
        if url.scheme() == "http" && !url.host().is_some_and(is_local) {
            return Err(ServerUrlError::Cleartext);
        }
        Ok(Self(url.as_str().trim_end_matches('/').to_owned()))
    }
}

/// A host on the person's own network, where plain HTTP stays at home: the
/// loopback, the private ranges (and the Android emulator's 10.0.2.2), a name
/// with no dot, or a `.local` one.
fn is_local(host: url::Host<&str>) -> bool {
    use url::Host;
    match host {
        Host::Ipv4(ip) => ip.is_loopback() || ip.is_private() || ip.is_link_local(),
        Host::Ipv6(ip) => ip.is_loopback(),
        Host::Domain(name) => {
            name.eq_ignore_ascii_case("localhost")
                || !name.contains('.')
                || name.to_ascii_lowercase().ends_with(".local")
        }
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
        for local in [
            "http://127.0.0.1:3000",
            "http://10.0.2.2:3000",
            "http://192.168.1.20:3000",
            "http://localhost:3000",
            "http://relay:3000",
            "http://relay.local:3000",
        ] {
            assert!(local.parse::<ServerUrl>().is_ok(), "{local}");
        }
        for public in ["http://relay.example.org", "http://8.8.8.8:3000"] {
            assert_eq!(
                public.parse::<ServerUrl>(),
                Err(ServerUrlError::Cleartext),
                "{public}"
            );
        }
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

    #[test]
    fn each_refusal_says_what_is_wrong() {
        let said = |text: &str| {
            text.parse::<ServerUrl>()
                .err()
                .map(|error| error.to_string())
        };
        assert_eq!(
            said("relay.example.org").as_deref(),
            Some("the server address must start with http:// or https://")
        );
        assert_eq!(
            said("https://").as_deref(),
            Some("the server address needs a host, such as https://relay.example.org")
        );
        assert_eq!(
            said("https://relay.example.org/?a=1").as_deref(),
            Some("the server address is only the relay's address, nothing after it")
        );
        assert_eq!(
            said("http://relay.example.org").as_deref(),
            Some("a server on the internet needs https://; http:// is only for your own network")
        );
        assert_eq!(
            said("ftp://relay.example.org").as_deref(),
            said("relay.example.org").as_deref()
        );
    }

    #[test]
    fn the_own_computer_over_ipv6_is_local_too() {
        let url: ServerUrl = "http://[::1]:3000".parse().expect("local");
        assert_eq!(url.as_str(), "http://[::1]:3000");
        assert!("http://[2001:db8::1]".parse::<ServerUrl>().is_err());
    }
}
