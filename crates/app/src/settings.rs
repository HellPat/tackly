//! This phone's own preferences: which filter the task lists show and the
//! color scheme. Kept in a small file next to the data; never synced.

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
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            filter: Filter::Mine,
            scheme: "ocean".into(),
        }
    }
}

fn file() -> PathBuf {
    platform::data_dir().join("settings.json")
}

impl Settings {
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
