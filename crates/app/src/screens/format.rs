//! Small helpers for showing times, durations and names.

use chrono::{DateTime, Utc};

/// "just now", "3 min ago", "2 h ago", "4 d ago".
pub fn ago(now: DateTime<Utc>, at: DateTime<Utc>) -> String {
    let seconds = (now - at).num_seconds().max(0);
    match seconds {
        0..=59 => "just now".into(),
        60..=3599 => format!("{} min ago", seconds / 60),
        3600..=86399 => format!("{} h ago", seconds / 3600),
        _ => format!("{} d ago", seconds / 86400),
    }
}

/// "42 s", "3 min 5 s".
pub fn duration(seconds: i64) -> String {
    if seconds < 60 {
        format!("{seconds} s")
    } else {
        format!("{} min {} s", seconds / 60, seconds % 60)
    }
}

/// The first letter of a name, for an avatar.
pub fn initial(name: &str) -> String {
    name.chars()
        .next()
        .map(|letter| letter.to_uppercase().to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_and_ages_read_naturally() {
        assert_eq!(duration(42), "42 s");
        assert_eq!(duration(185), "3 min 5 s");
        let now = Utc::now();
        assert_eq!(ago(now, now), "just now");
        assert_eq!(ago(now, now - chrono::Duration::minutes(3)), "3 min ago");
        assert_eq!(ago(now, now - chrono::Duration::hours(5)), "5 h ago");
    }

    #[test]
    fn initials_are_capitalised() {
        assert_eq!(initial("mona"), "M");
        assert_eq!(initial(""), "");
    }
}
