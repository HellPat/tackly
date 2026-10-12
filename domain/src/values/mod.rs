//! Values with rules: each can only be made from something valid, so the rest
//! of the code never checks again. Text that comes from a person or from
//! another phone is parsed into one at the edge (`parse`, `try_from`, or
//! deserializing an event).
//!
//! - [`Name`]: a name or title, not empty
//! - [`Photo`]: a small JPEG as a data URL
//! - [`GeoPoint`]: coordinates on the globe
//! - [`IconName`]: the name of an icon in the icon font
//! - [`Focus`]: one day's plan, at most [`MAX_FOCUS`] tasks

mod focus;
mod geo_point;
mod icon_name;
mod name;
mod photo;

pub use focus::{Focus, MAX_FOCUS, TooMuch};
pub use geo_point::{GeoPoint, GeoPointError};
pub use icon_name::{IconName, NotAnIconName};
pub use name::{EmptyName, Name};
pub use photo::{MAX_PHOTO_BYTES, Photo, PhotoError};
