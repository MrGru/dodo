//! Turning the engine's timestamps into a human "Last Started" relative time.
//!
//! The Engine API reports `State.StartedAt` as an RFC 3339 string in UTC
//! ([`parse_rfc3339_to_unix`] reads it without pulling in a date library), and
//! [`RelativeTime::since`] turns the gap between then and now into the coarsest
//! sensible unit. Both halves are pure so they are unit tested directly.

use crate::i18n::Str;

/// A "time ago" bucket, chosen so the label reads the way a person would say it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RelativeTime {
    /// The container has no recorded start (its `StartedAt` is unset or the zero
    /// value the engine uses for "never ran").
    Never,
    JustNow,
    Seconds(u64),
    Minutes(u64),
    Hours(u64),
    Days(u64),
    Weeks(u64),
    Months(u64),
    Years(u64),
}

impl RelativeTime {
    /// Buckets the gap between `started` and `now` (both Unix seconds). A missing
    /// or non-positive start is [`RelativeTime::Never`]; a future or sub-10s
    /// start reads as "just now" so a freshly started container is not "0
    /// seconds ago" or, worse, negative.
    pub fn since(started: Option<i64>, now: i64) -> Self {
        let Some(started) = started.filter(|&s| s > 0) else {
            return RelativeTime::Never;
        };
        let delta = now - started;
        if delta < 10 {
            return RelativeTime::JustNow;
        }
        let secs = delta as u64;
        const MINUTE: u64 = 60;
        const HOUR: u64 = 60 * MINUTE;
        const DAY: u64 = 24 * HOUR;
        const WEEK: u64 = 7 * DAY;
        // Approximate month/year lengths — a relative label does not need the
        // calendar to be exact, only to read right.
        const MONTH: u64 = 30 * DAY;
        const YEAR: u64 = 365 * DAY;

        if secs < MINUTE {
            RelativeTime::Seconds(secs)
        } else if secs < HOUR {
            RelativeTime::Minutes(secs / MINUTE)
        } else if secs < DAY {
            RelativeTime::Hours(secs / HOUR)
        } else if secs < WEEK {
            RelativeTime::Days(secs / DAY)
        } else if secs < MONTH {
            RelativeTime::Weeks(secs / WEEK)
        } else if secs < YEAR {
            RelativeTime::Months(secs / MONTH)
        } else {
            RelativeTime::Years(secs / YEAR)
        }
    }

    /// The localized label, carrying the count where there is one.
    pub fn label(self) -> Str {
        match self {
            RelativeTime::Never => Str::DockerRelNever,
            RelativeTime::JustNow => Str::DockerRelJustNow,
            RelativeTime::Seconds(n) => Str::DockerRelSecondsAgo(n),
            RelativeTime::Minutes(n) => Str::DockerRelMinutesAgo(n),
            RelativeTime::Hours(n) => Str::DockerRelHoursAgo(n),
            RelativeTime::Days(n) => Str::DockerRelDaysAgo(n),
            RelativeTime::Weeks(n) => Str::DockerRelWeeksAgo(n),
            RelativeTime::Months(n) => Str::DockerRelMonthsAgo(n),
            RelativeTime::Years(n) => Str::DockerRelYearsAgo(n),
        }
    }
}

/// Parses an RFC 3339 / ISO 8601 UTC timestamp (`2026-07-23T12:15:46.653Z`) into
/// Unix seconds. Only the fixed `YYYY-MM-DDTHH:MM:SS` prefix is read; the
/// fractional seconds and the trailing `Z` are ignored, because the engine
/// always reports UTC and a relative label needs no sub-second precision.
///
/// Returns `None` for anything that does not start with that shape — including
/// the engine's `0001-01-01T00:00:00Z` "never started" zero value, which lands
/// before the Unix epoch and so is reported as `None`.
pub fn parse_rfc3339_to_unix(s: &str) -> Option<i64> {
    let bytes = s.as_bytes();
    // Need at least "YYYY-MM-DDTHH:MM:SS".
    if bytes.len() < 19 {
        return None;
    }
    let num = |range: std::ops::Range<usize>| -> Option<i64> { s.get(range)?.parse().ok() };
    // Separators must be where we expect, or this is not the shape we parse.
    if &s[4..5] != "-" || &s[7..8] != "-" || &s[13..14] != ":" || &s[16..17] != ":" {
        return None;
    }
    let year = num(0..4)?;
    let month = num(5..7)?;
    let day = num(8..10)?;
    let hour = num(11..13)?;
    let minute = num(14..16)?;
    let second = num(17..19)?;

    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }

    let days = days_from_civil(year, month, day)?;
    let epoch = days * 86_400 + hour * 3_600 + minute * 60 + second;
    // Reject pre-epoch values (the "never" zero value among them).
    (epoch > 0).then_some(epoch)
}

/// Days since 1970-01-01 for a proleptic-Gregorian date, via Howard Hinnant's
/// `days_from_civil`. Returns `None` only if the fields are wildly out of range.
fn days_from_civil(year: i64, month: i64, day: i64) -> Option<i64> {
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400; // [0, 399]
    let doy = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    Some(era * 146_097 + doe - 719_468)
}

#[cfg(test)]
mod tests {
    use super::{RelativeTime, parse_rfc3339_to_unix};

    #[test]
    fn parses_a_known_utc_timestamp() {
        // 2021-01-01T00:00:00Z is 1609459200 in Unix seconds.
        assert_eq!(
            parse_rfc3339_to_unix("2021-01-01T00:00:00Z"),
            Some(1_609_459_200)
        );
    }

    #[test]
    fn ignores_fractional_seconds() {
        assert_eq!(
            parse_rfc3339_to_unix("2021-01-01T00:00:00.653044Z"),
            Some(1_609_459_200)
        );
    }

    #[test]
    fn the_epoch_itself_and_the_never_value_are_none() {
        // The engine's "never started" zero value is before the epoch.
        assert_eq!(parse_rfc3339_to_unix("0001-01-01T00:00:00Z"), None);
        // Garbage is None, not a panic.
        assert_eq!(parse_rfc3339_to_unix("not a date"), None);
        assert_eq!(parse_rfc3339_to_unix(""), None);
    }

    #[test]
    fn buckets_pick_the_coarsest_sensible_unit() {
        let now = 1_000_000_000;
        assert_eq!(RelativeTime::since(None, now), RelativeTime::Never);
        assert_eq!(RelativeTime::since(Some(0), now), RelativeTime::Never);
        // A start in the future or moments ago reads as "just now".
        assert_eq!(
            RelativeTime::since(Some(now + 5), now),
            RelativeTime::JustNow
        );
        assert_eq!(
            RelativeTime::since(Some(now - 3), now),
            RelativeTime::JustNow
        );
        assert_eq!(
            RelativeTime::since(Some(now - 30), now),
            RelativeTime::Seconds(30)
        );
        assert_eq!(
            RelativeTime::since(Some(now - 120), now),
            RelativeTime::Minutes(2)
        );
        assert_eq!(
            RelativeTime::since(Some(now - 3 * 3600), now),
            RelativeTime::Hours(3)
        );
        assert_eq!(
            RelativeTime::since(Some(now - 3 * 86_400), now),
            RelativeTime::Days(3)
        );
        assert_eq!(
            RelativeTime::since(Some(now - 2 * 7 * 86_400), now),
            RelativeTime::Weeks(2)
        );
        assert_eq!(
            RelativeTime::since(Some(now - 2 * 30 * 86_400), now),
            RelativeTime::Months(2)
        );
        assert_eq!(
            RelativeTime::since(Some(now - 3 * 365 * 86_400), now),
            RelativeTime::Years(3)
        );
    }
}
