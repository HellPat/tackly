//! Things that differ between the desktop test build and a phone.

use std::path::PathBuf;

use chrono::{DateTime, Local, NaiveDate};
use tackly_domain::GeoPoint;

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

/// The relay every build uses unless told otherwise.
const PRODUCTION_RELAY: &str = "https://relay.tackly.hellpat.com";

/// The relay a new family goes to unless the person picks another. In order:
/// `TACKLY_SERVER_URL` when the app starts, the address this build was made
/// for (`TACKLY_DEFAULT_SERVER` when it was built), and the production relay.
pub fn default_server() -> String {
    std::env::var("TACKLY_SERVER_URL")
        .ok()
        .or_else(|| option_env!("TACKLY_DEFAULT_SERVER").map(str::to_owned))
        .filter(|address| !address.trim().is_empty())
        .unwrap_or_else(|| PRODUCTION_RELAY.to_owned())
}

/// Asks Android for the location permission, once, right after creating or
/// joining a family, so the dialog never comes up while ticking a task off.
pub fn ask_for_location() {
    if cfg!(target_os = "android") && fixed_location().is_none() {
        dioxus::prelude::spawn(async {
            let _ = web_location().await;
        });
    }
}

/// Where the phone is now, for noting where a task was finished. Asked once
/// per tick, nothing in between. `TACKLY_LOCATION="lat,lon"` stands in for a
/// GPS (the desktop has none, and tests fix it). On a phone it is the web
/// view's geolocation: a fix up to a minute old is reused, otherwise it waits
/// up to half a minute for one (a cold GPS takes seconds).
pub async fn location() -> Option<GeoPoint> {
    if let Some(fixed) = fixed_location() {
        return Some(fixed);
    }
    if cfg!(target_os = "android") {
        web_location().await
    } else {
        None
    }
}

/// No permission or no fix: `None`.
async fn web_location() -> Option<GeoPoint> {
    const SCRIPT: &str = "navigator.geolocation.getCurrentPosition(\
        (p) => dioxus.send([p.coords.latitude, p.coords.longitude, p.coords.accuracy]),\
        () => dioxus.send(null),\
        { enableHighAccuracy: true, maximumAge: 60000, timeout: 30000 });";
    let fix: Option<[f64; 3]> = dioxus::document::eval(SCRIPT).recv().await.ok()?;
    let [latitude, longitude, accuracy] = fix?;
    GeoPoint::new(latitude, longitude, Some(accuracy)).ok()
}

fn fixed_location() -> Option<GeoPoint> {
    let fixed: GeoPoint = std::env::var("TACKLY_LOCATION").ok()?.parse().ok()?;
    GeoPoint::new(fixed.latitude(), fixed.longitude(), Some(10.0)).ok()
}

/// An invitation link passed on the command line (desktop). A phone gets it
/// from the system when the person taps the link; that is not wired up yet.
#[cfg(not(target_os = "android"))]
pub fn launch_link() -> Option<String> {
    std::env::args()
        .nth(1)
        .filter(|arg| arg.parse::<tackly_phone::InviteLink>().is_ok())
}

/// On Android the app is a library inside the Java process: there is no
/// command line, and `std::env::args()` panics ("capacity overflow").
#[cfg(target_os = "android")]
pub fn launch_link() -> Option<String> {
    None
}

/// The phone's local time. The acceptance tests move it (a test build only):
/// a file `clock-offset-minutes` in the data folder shifts it, so "it is
/// 20:00" and "the next morning" can be acted out.
pub fn local_now() -> DateTime<Local> {
    local(chrono::Utc::now())
}

/// A moment on this phone's clock (moved like [`local_now`] in a test build).
pub fn local(at: DateTime<chrono::Utc>) -> DateTime<Local> {
    let at = at.with_timezone(&Local);
    #[cfg(feature = "ui-test")]
    if let Some(minutes) = std::fs::read_to_string(data_dir().join("clock-offset-minutes"))
        .ok()
        .and_then(|text| text.trim().parse::<i64>().ok())
    {
        return at + chrono::Duration::minutes(minutes);
    }
    at
}

pub fn today() -> NaiveDate {
    local_now().date_naive()
}
