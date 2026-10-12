use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};

/// The largest photo a picture may carry, in bytes of its data URL. A 256 px
/// square JPEG is far smaller; this keeps a family's history small.
pub const MAX_PHOTO_BYTES: usize = 150_000;
const PREFIX: &str = "data:image/jpeg;base64,";

/// A small square JPEG as a data URL (`data:image/jpeg;base64,…`), made on
/// the phone from the camera or the gallery.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Photo(String);

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum PhotoError {
    #[error("a photo must be a JPEG data URL")]
    NotAJpeg,
    #[error("a photo must be small")]
    TooBig,
}

impl Photo {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for Photo {
    type Err = PhotoError;

    fn from_str(text: &str) -> Result<Self, PhotoError> {
        let data = text.strip_prefix(PREFIX).ok_or(PhotoError::NotAJpeg)?;
        let base64 = |b: u8| b.is_ascii_alphanumeric() || b"+/=".contains(&b);
        if data.is_empty() || !data.bytes().all(base64) {
            return Err(PhotoError::NotAJpeg);
        }
        if text.len() > MAX_PHOTO_BYTES {
            return Err(PhotoError::TooBig);
        }
        Ok(Self(text.to_owned()))
    }
}

impl TryFrom<String> for Photo {
    type Error = PhotoError;

    fn try_from(text: String) -> Result<Self, PhotoError> {
        text.parse()
    }
}

impl From<Photo> for String {
    fn from(photo: Photo) -> Self {
        photo.0
    }
}

impl fmt::Display for Photo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_small_jpeg_data_url_is_a_photo() {
        assert!(
            "data:image/jpeg;base64,/9j/4AAQSkZJRg=="
                .parse::<Photo>()
                .is_ok()
        );
    }

    #[test]
    fn anything_else_is_not() {
        for text in [
            "",
            "https://example.org/me.jpg",
            "data:image/png;base64,AAAA",
            "data:image/jpeg;base64,",
            "data:image/jpeg;base64,<script>",
        ] {
            assert_eq!(text.parse::<Photo>(), Err(PhotoError::NotAJpeg), "{text:?}");
        }
    }

    #[test]
    fn a_big_one_is_refused() {
        let big = format!("{PREFIX}{}", "A".repeat(MAX_PHOTO_BYTES));
        assert_eq!(big.parse::<Photo>(), Err(PhotoError::TooBig));
    }
}
