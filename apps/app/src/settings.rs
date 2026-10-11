//! This phone's own preferences: which filter the task lists show, the color
//! scheme, and when to plan the next day. Kept in a small file next to the
//! data; never synced.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::platform;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Filter {
    #[default]
    Mine,
    Unassigned,
    All,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub filter: Filter,
    pub scheme: String,
    /// When Upcoming opens to plan the next day ("20:00"); `None`: never.
    pub plan_at: Option<String>,
}

/// How many priorities a day holds.
pub const SLOTS: usize = 3;

impl Default for Settings {
    fn default() -> Self {
        Self {
            filter: Filter::Mine,
            scheme: "ocean".into(),
            // `TACKLY_PLAN_AT=off` (the acceptance tests) starts without planning.
            plan_at: match std::env::var("TACKLY_PLAN_AT").as_deref() {
                Ok("off") => None,
                Ok(time) if !time.is_empty() => Some(time.to_owned()),
                _ => Some("20:00".into()),
            },
        }
    }
}

fn file() -> PathBuf {
    platform::data_dir().join("settings.json")
}

impl Settings {
    /// The planning time, if one is set and it reads as HH:MM.
    pub fn plan_time(&self) -> Option<chrono::NaiveTime> {
        self.plan_at
            .as_deref()
            .and_then(|time| chrono::NaiveTime::parse_from_str(time, "%H:%M").ok())
    }

    /// The saved settings, or the defaults when there are none (or they can't be read).
    pub fn load() -> Self {
        std::fs::read(file())
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    /// Best effort: a preference that isn't saved is not worth an error message.
    pub fn save(&self) {
        if let Ok(json) = serde_json::to_vec_pretty(self) {
            let _ = std::fs::create_dir_all(platform::data_dir());
            let _ = std::fs::write(file(), json);
        }
    }
}
