//! The single injected clock. Every timestamp the build stage reads or writes
//! flows through a `Timestamp` handed in by the caller - `updated_at`,
//! the per-surface `surface_fetched_at` stamps, the reconcile staleness
//! warning, and the vaulting-soon horizon. Nothing in this crate may call
//! `Timestamp::now()` except the binary's arg parsing (when `--now` is absent).

use jiff::civil::{Date, DateTime};
use jiff::tz::TimeZone;
use jiff::Timestamp;
use market_domain::stamp::{days_between, parse_rfc3339, utc_midnight};

/// A UTC calendar day as days since 1970-01-01, the form
/// [`market_domain::math::StatsDay::day`] carries.
pub fn utc_day(date: Date) -> i64 {
    days_between(date, Date::constant(1970, 1, 1))
}

/// The day of a WFM statistics `datetime` (`2026-11-16T00:00:00.000+00:00`).
/// WFM stamps rows in UTC, so the leading date is the row's UTC day.
pub fn stats_day(datetime: &str) -> Option<i64> {
    let date = Date::strptime("%Y-%m-%d", datetime.get(..10)?).ok()?;
    Some(utc_day(date))
}

/// The snapshot's stamp format: UTC, whole seconds, `Z` suffix
/// (`%Y-%m-%dT%H:%M:%SZ`).
pub fn iso_z(dt: Timestamp) -> String {
    dt.strftime("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// Strict `%Y-%m-%dT%H:%M:%SZ` parse -
/// used on prior `surface_fetched_at` stamps. Anything else is a parse
/// failure.
pub fn parse_stamp(s: &str) -> Option<Timestamp> {
    naive_utc(DateTime::strptime("%Y-%m-%dT%H:%M:%SZ", s).ok()?)
}

/// Epoch milliseconds → UTC. DE's worldState stamps every window
/// (`Activation`, `Expiry`) as millis inside a Mongo-flavoured wrapper; once
/// [`crate::ingest::de::de_millis`] has unwrapped it, this is the only conversion
/// needed. Out-of-range values clamp to the epoch rather than panicking - a
/// garbage timestamp should render as obviously wrong, not take down a build.
pub fn from_millis(ms: i64) -> Timestamp {
    Timestamp::from_millisecond(ms).unwrap_or(Timestamp::UNIX_EPOCH)
}

/// The ISO 8601 subset the build stage is fed
/// (WFCD `estimatedVaultDate`, after the caller's `Z` → `+00:00` rewrite):
/// RFC 3339 with offset, naive datetime with optional fraction (T or space
/// separator), or a bare date. Naive values are assumed UTC.
pub fn parse_isoformat_utc(s: &str) -> Option<Timestamp> {
    if let Some(dt) = parse_rfc3339(s) {
        return Some(dt);
    }
    for fmt in [
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%dT%H:%M",
        "%Y-%m-%d %H:%M",
    ] {
        if let Ok(n) = DateTime::strptime(fmt, s) {
            return naive_utc(n);
        }
    }
    utc_midnight(Date::strptime("%Y-%m-%d", s).ok()?)
}

/// A naive datetime read as UTC.
pub fn naive_utc(dt: DateTime) -> Option<Timestamp> {
    dt.to_zoned_datetime(TimeZone::UTC)
        .ok()
        .map(|z| z.timestamp())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utc(y: i16, mo: i8, d: i8, h: i8, mi: i8, s: i8) -> Timestamp {
        naive_utc(jiff::civil::datetime(y, mo, d, h, mi, s, 0)).unwrap()
    }

    #[test]
    fn days_count_from_the_epoch() {
        assert_eq!(utc_day(Date::constant(1970, 1, 1)), 0);
        assert_eq!(
            utc_day(market_domain::stamp::utc_date(utc(2026, 11, 17, 23, 59, 0))),
            20_774
        );
        assert_eq!(stats_day("2026-11-16T00:00:00.000+00:00"), Some(20_773));
        assert_eq!(stats_day("2026-11"), None);
        assert_eq!(stats_day("yesterday at noon"), None);
    }

    #[test]
    fn iso_z_round_trips_through_parse_stamp() {
        let t = utc(2026, 7, 1, 12, 34, 56);
        assert_eq!(iso_z(t), "2026-07-01T12:34:56Z");
        assert_eq!(parse_stamp(&iso_z(t)), Some(t));
    }

    #[test]
    fn parse_stamp_rejects_non_stamp_formats() {
        assert_eq!(parse_stamp("2026-07-01"), None);
        assert_eq!(parse_stamp("2026-07-01T12:34:56+00:00"), None);
        assert_eq!(parse_stamp("garbage"), None);
    }

    #[test]
    fn isoformat_accepts_wfcd_shapes() {
        // The real WFCD shape after the caller's Z→+00:00 rewrite.
        assert_eq!(
            parse_isoformat_utc("2021-07-08T00:00:00.000+00:00"),
            Some(utc(2021, 7, 8, 0, 0, 0))
        );
        // Naive datetime → assumed UTC.
        assert_eq!(
            parse_isoformat_utc("2026-07-25T10:30:00"),
            Some(utc(2026, 7, 25, 10, 30, 0))
        );
        // Bare date → midnight UTC.
        assert_eq!(
            parse_isoformat_utc("2020-08-11"),
            Some(utc(2020, 8, 11, 0, 0, 0))
        );
        // Non-UTC offset is honored.
        assert_eq!(
            parse_isoformat_utc("2026-01-01T02:00:00+02:00"),
            Some(utc(2026, 1, 1, 0, 0, 0))
        );
    }

    #[test]
    fn isoformat_rejects_garbage() {
        assert_eq!(parse_isoformat_utc("soon"), None);
        assert_eq!(parse_isoformat_utc(""), None);
    }
}

#[cfg(test)]
mod de_millis_tests {
    use super::*;

    #[test]
    fn from_millis_matches_a_known_worldstate_stamp() {
        // Baro's Activation from the 2026-08 worldState probe.
        assert_eq!(iso_z(from_millis(1787317200000)), "2026-08-21T13:00:00Z");
    }

    #[test]
    fn from_millis_clamps_instead_of_panicking() {
        assert_eq!(from_millis(i64::MAX), Timestamp::UNIX_EPOCH);
    }
}
