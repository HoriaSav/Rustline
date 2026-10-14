//! GTFS times.
//!
//! GTFS measures times from "noon minus 12 hours" of the service day and lets
//! them go past 24:00 for trips that run after midnight (e.g. `25:10:00`).
//! A clock type like `chrono::NaiveTime` cannot hold that, so Rustline uses
//! plain seconds.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Seconds since the service day's "noon minus 12h". May exceed 86 400.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Seconds(pub u32);

impl Seconds {
    /// Builds a time from hours, minutes and seconds (hours may be ≥ 24).
    ///
    /// Panics on overflow in debug builds; keep `h` small (`parse_gtfs_time` allows at most 3 digits).
    pub fn hms(h: u32, m: u32, s: u32) -> Self {
        Self(h * 3600 + m * 60 + s)
    }
}

/// Formats as `HH:MM:SS`, keeping hours ≥ 24 (e.g. `25:10:00`).
impl fmt::Display for Seconds {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (h, m, s) = (self.0 / 3600, self.0 / 60 % 60, self.0 % 60);
        write!(f, "{h:02}:{m:02}:{s:02}")
    }
}

/// Why a GTFS time string could not be parsed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TimeError {
    /// Not of the form `H:MM:SS` / `HH:MM:SS`.
    #[error("invalid GTFS time {0:?}: expected H:MM:SS")]
    Format(String),
    /// Minutes or seconds ≥ 60.
    #[error("invalid GTFS time {0:?}: minutes and seconds must be < 60")]
    Range(String),
}

/// Parses a GTFS time such as `"8:05:00"`, `"08:05:00"` or `"25:10:00"`.
///
/// Leading/trailing spaces are ignored. Hours may have one or more digits;
/// minutes and seconds must have exactly two.
pub fn parse_gtfs_time(s: &str) -> Result<Seconds, TimeError> {
    let t = s.trim();
    let err = || TimeError::Format(s.to_owned());
    let mut parts = t.split(':');
    let (Some(h), Some(m), Some(sec), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(err());
    };
    let digits = |p: &str| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit());
    if !digits(h) || h.len() > 3 || m.len() != 2 || sec.len() != 2 || !digits(m) || !digits(sec) {
        return Err(err());
    }
    // The checks above guarantee these parse.
    let (h, m, sec): (u32, u32, u32) =
        (h.parse().unwrap(), m.parse().unwrap(), sec.parse().unwrap());
    if m >= 60 || sec >= 60 {
        return Err(TimeError::Range(s.to_owned()));
    }
    Ok(Seconds::hms(h, m, sec))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_digit_hour() {
        assert_eq!(parse_gtfs_time("8:05:00"), Ok(Seconds(8 * 3600 + 5 * 60)));
    }

    #[test]
    fn two_digit_hour() {
        assert_eq!(parse_gtfs_time("08:05:00"), Ok(Seconds(29_100)));
    }

    #[test]
    fn after_midnight() {
        assert_eq!(parse_gtfs_time("25:10:00"), Ok(Seconds(90_600)));
        assert_eq!(parse_gtfs_time("24:00:00"), Ok(Seconds(86_400)));
    }

    #[test]
    fn midnight_and_spaces() {
        assert_eq!(parse_gtfs_time("00:00:00"), Ok(Seconds(0)));
        assert_eq!(parse_gtfs_time(" 7:00:30 "), Ok(Seconds(25_230)));
    }

    #[test]
    fn rejects_garbage() {
        for bad in [
            "",
            "abc",
            "8:05",
            "8:5:00",
            "08:05:00:00",
            "-1:00:00",
            "8:05:0x",
            "1234:00:00",
        ] {
            assert!(
                matches!(parse_gtfs_time(bad), Err(TimeError::Format(_))),
                "{bad:?} should be a format error"
            );
        }
    }

    #[test]
    fn rejects_out_of_range() {
        assert_eq!(
            parse_gtfs_time("08:60:00"),
            Err(TimeError::Range("08:60:00".into()))
        );
        assert_eq!(
            parse_gtfs_time("08:00:61"),
            Err(TimeError::Range("08:00:61".into()))
        );
    }

    #[test]
    fn displays_past_24h() {
        assert_eq!(Seconds(90_600).to_string(), "25:10:00");
    }
}
