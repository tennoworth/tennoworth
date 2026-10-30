pub(crate) mod announcements;
pub(crate) mod browser;
pub(crate) mod game_log;
pub(crate) mod inventory;
pub(crate) mod market;
pub(crate) mod notifications;
pub(crate) mod telemetry;
pub(crate) mod trading;
pub(crate) mod watches;
pub(crate) mod wfm;

pub(crate) fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    #[test]
    fn unix_now_tracks_epoch_seconds() {
        let before = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        let now = super::unix_now();
        let after = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        assert!((before..=after).contains(&now));
    }
}
