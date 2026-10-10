//! Things that differ between the desktop test build and a phone.

use std::path::PathBuf;

use tackly_protocol::GeoPoint;

pub fn data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("TACKLY_DATA_DIR") {
        return dir.into();
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    home.join(".tackly")
}

pub fn default_server() -> String {
    std::env::var("TACKLY_SERVER_URL").unwrap_or_else(|_| "http://127.0.0.1:3000".into())
}

/// Desktop has no GPS. `TACKLY_LOCATION="lat,lon"` stands in for it so the
/// completion metadata can be tried; a phone build reads the real sensor.
pub fn location() -> Option<GeoPoint> {
    let raw = std::env::var("TACKLY_LOCATION").ok()?;
    let (lat, lon) = raw.split_once(',')?;
    Some(GeoPoint {
        latitude: lat.trim().parse().ok()?,
        longitude: lon.trim().parse().ok()?,
        accuracy_meters: Some(10.0),
    })
}

/// An invitation link passed on the command line (a phone gets it from the
/// system when the person taps the link).
pub fn launch_link() -> Option<String> {
    std::env::args()
        .nth(1)
        .filter(|arg| arg.starts_with("tackly://"))
}
