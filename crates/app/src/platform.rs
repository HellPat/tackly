//! Things that differ between the desktop test build and a phone.

use std::path::PathBuf;

use tackly_protocol::GeoPoint;

pub fn data_dir() -> PathBuf {
    match std::env::var_os("TACKLY_DATA_DIR") {
        Some(dir) => dir.into(),
        None => default_data_dir(),
    }
}

#[cfg(target_os = "android")]
fn default_data_dir() -> PathBuf {
    // The app-private directory: /data/data/<package>/files. The package name
    // is the process name.
    let cmdline = std::fs::read("/proc/self/cmdline").unwrap_or_default();
    let cmdline = String::from_utf8_lossy(&cmdline);
    let package = cmdline.split('\0').next().unwrap_or("dev.tackly.tackly");
    let dir = PathBuf::from("/data/data").join(package).join("files");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

#[cfg(not(target_os = "android"))]
fn default_data_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join(".tackly")
}

pub fn default_server() -> String {
    // The emulator reaches the computer it runs on at 10.0.2.2.
    let fallback = if cfg!(target_os = "android") {
        "http://10.0.2.2:3000"
    } else {
        "http://127.0.0.1:3000"
    };
    std::env::var("TACKLY_SERVER_URL").unwrap_or_else(|_| fallback.into())
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

/// An invitation link passed on the command line (desktop). A phone gets it
/// from the system when the person taps the link; that is not wired up yet.
#[cfg(not(target_os = "android"))]
pub fn launch_link() -> Option<String> {
    std::env::args()
        .nth(1)
        .filter(|arg| arg.starts_with("tackly://"))
}

/// On Android the app is a library inside the Java process: there is no
/// command line, and `std::env::args()` panics ("capacity overflow").
#[cfg(target_os = "android")]
pub fn launch_link() -> Option<String> {
    None
}
