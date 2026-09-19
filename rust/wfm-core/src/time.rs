pub fn chrono_now_iso() -> String {
    iso_from(std::time::SystemTime::now())
}

/// The wire's ISO-8601 UTC stamp for an observation instant. Whole seconds are
/// deliberate: every freshness rule reading this works in whole seconds, and a
/// coarser stamp can never look fresher than the instant it describes.
pub fn iso_from(at: std::time::SystemTime) -> String {
    // We don't pull in chrono just for this; format manually from SystemTime.
    use std::time::UNIX_EPOCH;
    let secs = at
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days_since_epoch = secs / 86400;
    let secs_in_day = secs % 86400;
    let h = secs_in_day / 3600;
    let m = (secs_in_day / 60) % 60;
    let s = secs_in_day % 60;
    let (y, mo, d) = civil_from_days(days_since_epoch as i64);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{m:02}:{s:02}Z")
}

// Howard Hinnant's algorithm - converts days-since-epoch to (year, month, day).
pub fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}
