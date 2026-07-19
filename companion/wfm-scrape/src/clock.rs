//! The single injected clock. Every timestamp the converter reads or writes
//! flows through a `DateTime<Utc>` handed in by the caller — `updated_at`,
//! the per-surface `surface_fetched_at` stamps, the reconcile staleness
//! warning, and the vaulting-soon horizon. Nothing in this crate may call
//! `Utc::now()` except the binary's arg parsing (when `--now` is absent).

use chrono::{DateTime, NaiveDate, NaiveDateTime, TimeZone, Utc};

/// The snapshot's stamp format — Python's
/// `datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")`.
pub fn iso_z(dt: DateTime<Utc>) -> String {
    dt.format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// Strict mirror of the converter's `strptime(s, "%Y-%m-%dT%H:%M:%SZ")` —
/// used on prior `surface_fetched_at` stamps. Anything else is a parse
/// failure, exactly like Python's ValueError path.
pub fn parse_stamp(s: &str) -> Option<DateTime<Utc>> {
    NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%SZ")
        .ok()
        .map(|n| Utc.from_utc_datetime(&n))
}

/// Mirror of the `datetime.fromisoformat` subset the converter feeds it
/// (WFCD `estimatedVaultDate`, after the caller's `Z` → `+00:00` rewrite):
/// RFC 3339 with offset, naive datetime with optional fraction (T or space
/// separator), or a bare date. Naive values are assumed UTC, matching the
/// explicit `tzinfo is None → utc` branch in `fetch_vault_status`.
pub fn parse_isoformat_utc(s: &str) -> Option<DateTime<Utc>> {
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Some(dt.with_timezone(&Utc));
    }
    for fmt in [
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%dT%H:%M",
        "%Y-%m-%d %H:%M",
    ] {
        if let Ok(n) = NaiveDateTime::parse_from_str(s, fmt) {
            return Some(Utc.from_utc_datetime(&n));
        }
    }
    let d = NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()?;
    Some(Utc.from_utc_datetime(&d.and_hms_opt(0, 0, 0)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utc(y: i32, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, mo, d, h, mi, s).unwrap()
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
        assert_eq!(parse_isoformat_utc("2020-08-11"), Some(utc(2020, 8, 11, 0, 0, 0)));
        // Non-UTC offset is honored.
        assert_eq!(
            parse_isoformat_utc("2026-01-01T02:00:00+02:00"),
            Some(utc(2026, 1, 1, 0, 0, 0))
        );
    }

    #[test]
    fn isoformat_rejects_garbage_like_python_valueerror() {
        assert_eq!(parse_isoformat_utc("soon"), None);
        assert_eq!(parse_isoformat_utc(""), None);
    }
}
