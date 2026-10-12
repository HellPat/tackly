//! This phone's own preferences: which filter the task lists show, the color
//! scheme, and when to plan the next day. Kept in a small file next to the
//! data; never synced.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::{platform, theme::SchemeId};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Filter {
    #[default]
    Mine,
    Unassigned,
    All,
}

/// A time of day, to the minute: when Upcoming opens to plan the next day.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct TimeOfDay(chrono::NaiveTime);

#[derive(Debug, PartialEq, Eq)]
pub struct NotATime;

impl std::fmt::Display for NotATime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("a time is written as hours and minutes, like 20:00")
    }
}

impl std::error::Error for NotATime {}

impl TimeOfDay {
    pub fn time(self) -> chrono::NaiveTime {
        self.0
    }
}

impl std::str::FromStr for TimeOfDay {
    type Err = NotATime;

    fn from_str(text: &str) -> Result<Self, NotATime> {
        chrono::NaiveTime::parse_from_str(text.trim(), "%H:%M")
            .map(Self)
            .map_err(|_| NotATime)
    }
}

impl TryFrom<String> for TimeOfDay {
    type Error = NotATime;

    fn try_from(text: String) -> Result<Self, NotATime> {
        text.parse()
    }
}

impl From<TimeOfDay> for String {
    fn from(time: TimeOfDay) -> Self {
        time.to_string()
    }
}

impl std::fmt::Display for TimeOfDay {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.format("%H:%M"))
    }
}

/// What is kept in the file. Each preference is read on its own: one that
/// cannot be read (an old or damaged value) falls back to its default and
/// does not take the others with it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(from = "RawSettings")]
pub struct Settings {
    pub filter: Filter,
    pub scheme: SchemeId,
    /// When Upcoming opens to plan the next day; `None`: never.
    pub plan_at: Option<TimeOfDay>,
}

/// The file as it is: a map of preferences, each read on its own.
#[derive(Deserialize)]
#[serde(transparent)]
struct RawSettings(serde_json::Map<String, serde_json::Value>);

impl From<RawSettings> for Settings {
    fn from(raw: RawSettings) -> Self {
        fn read<T: serde::de::DeserializeOwned>(raw: &RawSettings, name: &str) -> Option<T> {
            raw.0
                .get(name)
                .and_then(|value| serde_json::from_value(value.clone()).ok())
        }
        let defaults = Self::default();
        Self {
            filter: read(&raw, "filter").unwrap_or(defaults.filter),
            scheme: read(&raw, "scheme").unwrap_or(defaults.scheme),
            // A saved `null` is a choice: never.
            plan_at: match raw.0.get("plan_at") {
                Some(serde_json::Value::Null) => None,
                _ => read(&raw, "plan_at").or(defaults.plan_at),
            },
        }
    }
}

/// How many priorities a day holds.
pub const SLOTS: usize = 3;

impl Default for Settings {
    fn default() -> Self {
        Self {
            filter: Filter::Mine,
            scheme: SchemeId::default(),
            // `TACKLY_PLAN_AT=off` (the acceptance tests) starts without planning.
            plan_at: match std::env::var("TACKLY_PLAN_AT").as_deref() {
                Ok("off") => None,
                Ok(time) if !time.is_empty() => time.parse().ok(),
                _ => "20:00".parse().ok(),
            },
        }
    }
}

fn file() -> PathBuf {
    platform::data_dir().join("settings.json")
}

impl Settings {
    /// The planning time, if one is set.
    pub fn plan_time(&self) -> Option<chrono::NaiveTime> {
        self.plan_at.map(TimeOfDay::time)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_time_of_day_is_hours_and_minutes() {
        assert_eq!(
            " 19:05 ".parse::<TimeOfDay>().map(|time| time.to_string()),
            Ok("19:05".to_owned())
        );
        for text in ["", "7", "25:00", "20:61", "twenty"] {
            assert_eq!(text.parse::<TimeOfDay>(), Err(NotATime), "{text:?}");
        }
    }

    #[test]
    fn each_preference_is_read_on_its_own() {
        let read = |json: &str| serde_json::from_str::<Settings>(json).expect("settings");
        let defaults = Settings::default();

        let full = read(r#"{"filter":"all","scheme":"lagoon","plan_at":"18:30"}"#);
        assert_eq!(full.filter, Filter::All);
        assert_eq!(full.scheme.to_string(), "lagoon");
        assert_eq!(
            full.plan_at.map(|time| time.to_string()).as_deref(),
            Some("18:30")
        );

        // A value that cannot be read costs only itself.
        let damaged = read(r#"{"filter":"all","scheme":"no-such-scheme","plan_at":"soon"}"#);
        assert_eq!(damaged.filter, Filter::All);
        assert_eq!(damaged.scheme, defaults.scheme);
        assert_eq!(damaged.plan_at, defaults.plan_at);

        // Never planning is a choice that is kept.
        assert_eq!(read(r#"{"plan_at":null}"#).plan_at, None);
        // Nothing saved: the defaults.
        assert_eq!(read("{}"), defaults);
    }
}
