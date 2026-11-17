//! The opt-in collectors that run on the host: the daily installation count
//! (`usage`) and anonymous price and riven-shape reports (`reports`). They share
//! the request quota and serve loop in `gate`, and nothing else: each binary is
//! its own process with its own database and quota, so neither can starve the
//! other.

pub mod gate;
pub mod reports;
pub mod usage;

use jiff::civil::Date;
use jiff::tz::TimeZone;
use jiff::Timestamp;

/// The UTC calendar day of an instant.
pub(crate) fn utc_date(at: Timestamp) -> Date {
    at.to_zoned_datetime(TimeZone::UTC).date()
}

/// RFC 3339 in UTC with a `+00:00` offset and a 0, 3, 6 or 9 digit fraction:
/// the shape the published `updated_at` has always had, which the usage page
/// prints verbatim.
pub(crate) fn rfc3339(at: Timestamp) -> String {
    let nanos = at.subsec_nanosecond();
    let fraction = if nanos == 0 {
        ""
    } else if nanos % 1_000_000 == 0 {
        "%.3f"
    } else if nanos % 1_000 == 0 {
        "%.6f"
    } else {
        "%.9f"
    };
    at.strftime(&format!("%Y-%m-%dT%H:%M:%S{fraction}+00:00"))
        .to_string()
}

/// `n` days after `date`, or `None` past the calendar's range.
pub(crate) fn add_days(date: Date, n: i64) -> Option<Date> {
    date.checked_add(jiff::SignedDuration::from_hours(n * 24))
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc3339_keeps_the_published_shape() {
        let at = |s: &str| rfc3339(s.parse().unwrap());
        assert_eq!(at("2026-11-17T12:00:00Z"), "2026-11-17T12:00:00+00:00");
        assert_eq!(
            at("2026-11-17T12:00:00.12Z"),
            "2026-11-17T12:00:00.120+00:00"
        );
        assert_eq!(
            at("2026-11-17T12:00:00.000250Z"),
            "2026-11-17T12:00:00.000250+00:00"
        );
        assert_eq!(
            at("2026-11-17T12:00:00.000000001Z"),
            "2026-11-17T12:00:00.000000001+00:00"
        );
        assert_eq!(at("2026-11-17T13:00:00+01:00"), "2026-11-17T12:00:00+00:00");
    }
}
