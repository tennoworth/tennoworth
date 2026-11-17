//! Strict RFC 3339 instants, the form every snapshot `updated_at` and market
//! stamp takes. jiff's own `Timestamp` parser accepts the wider ISO 8601 /
//! RFC 9557 family (`+0530`, `+05`, `,5`, `[Europe/Paris]`), which would let a
//! malformed snapshot pass the market contract, so the shape is checked here
//! before jiff reads the value.

use jiff::civil::Date;
use jiff::tz::TimeZone;
use jiff::{SignedDuration, Timestamp};

/// `YYYY-MM-DD`, a `T`, `t` or space, `HH:MM:SS`, an optional fraction, then
/// `Z`, `z` or `±HH:MM`. Fraction digits past nanoseconds are dropped.
pub fn parse_rfc3339(s: &str) -> Option<Timestamp> {
    let b = s.as_bytes();
    let digits = |from: usize, to: usize| {
        b.get(from..to)
            .is_some_and(|d| d.iter().all(u8::is_ascii_digit))
    };
    let at = |i: usize, c: &[u8]| b.get(i).is_some_and(|x| c.contains(x));
    let shape = digits(0, 4)
        && at(4, b"-")
        && digits(5, 7)
        && at(7, b"-")
        && digits(8, 10)
        && at(10, b"Tt ")
        && digits(11, 13)
        && at(13, b":")
        && digits(14, 16)
        && at(16, b":")
        && digits(17, 19);
    if !shape {
        return None;
    }
    let mut end = 19;
    let mut fraction = "";
    if at(19, b".") {
        end = 20
            + b.get(20..)?
                .iter()
                .take_while(|c| c.is_ascii_digit())
                .count();
        if end == 20 {
            return None;
        }
        fraction = s.get(20..end.min(29))?;
    }
    let offset = s.get(end..)?;
    let zone = match offset.as_bytes() {
        [b'Z' | b'z'] => "Z".to_owned(),
        [b'+' | b'-', h1, h2, b':', m1, m2]
            if [h1, h2, m1, m2].iter().all(|c| c.is_ascii_digit()) =>
        {
            // Offsets of a day or more are not RFC 3339 offsets.
            let hours = u32::from(h1 - b'0') * 10 + u32::from(h2 - b'0');
            let minutes = u32::from(m1 - b'0') * 10 + u32::from(m2 - b'0');
            if hours > 23 || minutes > 59 {
                return None;
            }
            offset.to_owned()
        }
        _ => return None,
    };
    let date = s.get(0..10)?;
    let time = s.get(11..19)?;
    let normal = if fraction.is_empty() {
        format!("{date}T{time}{zone}")
    } else {
        format!("{date}T{time}.{fraction}{zone}")
    };
    normal.parse().ok()
}

/// The UTC calendar day of an instant.
pub fn utc_date(at: Timestamp) -> Date {
    at.to_zoned_datetime(TimeZone::UTC).date()
}

/// Midnight UTC opening `date`.
pub fn utc_midnight(date: Date) -> Option<Timestamp> {
    date.to_zoned_datetime(TimeZone::UTC)
        .ok()
        .map(|z| z.timestamp())
}

/// `n` whole days. Panics on overflow, as `Date + days` does.
pub const fn days(n: i64) -> SignedDuration {
    SignedDuration::from_hours(n * 24)
}

/// Whole days from `earlier` to `later`, truncated toward zero.
pub fn days_between(later: Date, earlier: Date) -> i64 {
    later.duration_since(earlier).as_hours() / 24
}

/// Whole days elapsed between two instants, truncated toward zero.
pub fn whole_days(later: Timestamp, earlier: Timestamp) -> i64 {
    later.duration_since(earlier).as_hours() / 24
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secs(s: &str) -> Option<(i64, i32)> {
        parse_rfc3339(s).map(|t| (t.as_second(), t.subsec_nanosecond()))
    }

    #[test]
    fn accepts_the_rfc_3339_forms() {
        let noon = Some((1_794_916_800, 0));
        assert_eq!(secs("2026-11-17T12:00:00Z"), noon);
        assert_eq!(secs("2026-11-17t12:00:00z"), noon);
        assert_eq!(secs("2026-11-17 12:00:00Z"), noon);
        assert_eq!(secs("2026-11-17T12:00:00+00:00"), noon);
        assert_eq!(secs("2026-11-17T12:00:00-00:00"), noon);
        assert_eq!(secs("2026-11-17T13:00:00+01:00"), noon);
        assert_eq!(
            secs("2026-11-17T12:00:00.250Z"),
            Some((1_794_916_800, 250_000_000))
        );
        assert_eq!(
            secs("2026-11-17T12:00:00.1234567891Z"),
            Some((1_794_916_800, 123_456_789))
        );
    }

    #[test]
    fn rejects_iso_8601_forms_outside_rfc_3339() {
        for s in [
            "2026-11-17T12:00:00+0530",
            "2026-11-17T12:00:00+05",
            "2026-11-17T12:00:00+24:00",
            "2026-11-17T12:00:00-23:59:59",
            "2026-11-17T12:00:00Z[UTC]",
            "2026-11-17T12:00:00+00:00[Europe/Paris]",
            "2026-11-17T12:00:00,5Z",
            "2026-11-17T12:00:00.Z",
            "2026-11-17T12:00Z",
            "2026-11-17T12:00:00",
            "20261117T120000Z",
            "2026-1-17T12:00:00Z",
            "2026-11-17",
            "2026-13-40T10:00:00Z",
            " 2026-11-17T12:00:00Z",
            "2026-11-17T12:00:00Z ",
            "+002026-11-17T12:00:00Z",
            "yesterday",
            "",
        ] {
            assert_eq!(secs(s), None, "{s}");
        }
    }

    #[test]
    fn utc_date_is_the_utc_day() {
        let late = parse_rfc3339("2026-11-17T23:30:00-05:00").unwrap();
        assert_eq!(utc_date(late), Date::constant(2026, 11, 18));
    }
}
