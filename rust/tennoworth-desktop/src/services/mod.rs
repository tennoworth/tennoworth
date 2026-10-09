pub(crate) mod acquisition;
pub(crate) mod allowance;
pub(crate) mod auto_scan;
pub(crate) mod browser;
pub(crate) mod definitions;
pub(crate) mod eelog;
pub(crate) mod eelog_state;
pub(crate) mod inventory;
pub(crate) mod market;
pub(crate) mod notifications;
pub(crate) mod order_mutations;
pub(crate) mod presence;
pub(crate) mod protection;
pub(crate) mod recording;
pub(crate) mod reminders;
pub(crate) mod sellables;
pub(crate) mod trades;
pub(crate) mod watch;
pub(crate) mod wfm_session;
pub(crate) mod wfm_signin;
pub(crate) mod ws_watch;

pub(crate) mod reports;
pub(crate) mod usage;

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
