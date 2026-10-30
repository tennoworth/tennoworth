//! Curated, expiring messages. Remote prose is data, never an instruction to trade.
use super::{market::cache::MarketCache, notifications};
use crate::contracts::notification::{AnnouncementNotification, Candidate, NotificationContent};
use announcement_feed::stamp;
pub use announcement_feed::{parse, source_allowed, validated_stamp, AnnouncementFeed};
use tauri::{AppHandle, Manager};

pub const EVENT: &str = "announcements-changed";
const BUNDLED: &str = include_str!("../../../../frontend/public/announcements.json");

pub fn load(cache: &MarketCache) -> Option<AnnouncementFeed> {
    let cached = cache.cached_announcements().and_then(|raw| parse(&raw));
    let bundled = parse(BUNDLED);
    match (cached, bundled) {
        (Some(cached), Some(bundled)) if cached.updated_at >= bundled.updated_at => Some(cached),
        (_, Some(bundled)) => Some(bundled),
        (cached, _) => cached,
    }
}

pub fn candidates(feed: &AnnouncementFeed, now: i64) -> Vec<Candidate> {
    feed.messages
        .iter()
        .filter_map(|message| {
            let start = stamp(&message.starts_at)?;
            let end = stamp(&message.ends_at)?;
            if now < start || now >= end {
                return None;
            }
            let mut candidate = Candidate::once(
                format!("announcement:{}", message.id),
                "announcements",
                message.title.clone(),
                message.body.clone(),
                "sell",
                now,
            );
            // Clearing history or moving a deadline must never repeat the same ID.
            candidate.expires_at = i64::MAX;
            candidate.content = Some(NotificationContent::Announcement(
                AnnouncementNotification {
                    source_url: message.source_url.clone(),
                    ends_at: message.ends_at.clone(),
                },
            ));
            Some(candidate)
        })
        .collect()
}

/// Popups use only a feed fetched from the site. The bundled copy can still carry
/// a message withdrawn since the release, which an offline first launch must not
/// announce; the banner falls back to it only until those messages expire.
fn fetched(cache: &MarketCache) -> Option<AnnouncementFeed> {
    cache.cached_announcements().and_then(|raw| parse(&raw))
}

pub fn deliver(app: &AppHandle) {
    if let Some(feed) = fetched(&app.state::<MarketCache>()) {
        for candidate in candidates(&feed, notifications::now()) {
            match notifications::publish(app, candidate) {
                Ok(true) => break,
                Ok(false) => {}
                Err(error) => {
                    eprintln!("tennoworth: announcement could not be recorded: {error}");
                    break;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persistence::Db;

    #[test]
    fn announcement_contract_matches_shared_cases() {
        let cases: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../tests/fixtures/announcements/cases.json"
        ))
        .unwrap();
        for case in cases.as_array().unwrap() {
            let feed = parse(&case["feed"].to_string());
            assert_eq!(
                feed.is_some(),
                case["valid"].as_bool().unwrap(),
                "{}",
                case["name"]
            );
            let ids: Vec<_> = feed
                .as_ref()
                .map(|feed| {
                    candidates(feed, stamp(case["now"].as_str().unwrap()).unwrap())
                        .into_iter()
                        .map(|c| c.key.strip_prefix("announcement:").unwrap().to_string())
                        .collect()
                })
                .unwrap_or_default();
            assert_eq!(
                serde_json::to_value(ids).unwrap(),
                case["active_ids"],
                "{}",
                case["name"]
            );
        }
        assert!(parse(BUNDLED).is_some());
    }

    #[test]
    fn announcements_do_not_repeat_after_restart_or_clearing_history() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("notifications.sqlite");
        let feed = parse(BUNDLED).unwrap();
        let now = stamp(&feed.messages[0].starts_at).unwrap();
        let candidate = candidates(&feed, now).remove(0);
        let db = Db::open(&path).unwrap();
        assert!(db
            .insert_notification(&candidate, now, true, true)
            .unwrap()
            .is_some());
        assert_eq!(db.list_notifications().unwrap()[0].delivery, "pending");
        db.clear_notifications().unwrap();
        db.prune_notifications(now + 365 * 86400).unwrap();
        drop(db);
        let db = Db::open(&path).unwrap();
        assert!(db
            .insert_notification(&candidate, now + 60, true, true)
            .unwrap()
            .is_none());
        assert!(db.list_notifications().unwrap().is_empty());
    }

    #[test]
    fn disabled_announcements_are_checkpointed_without_a_popup_or_inbox_entry() {
        let db = Db::open_in_memory().unwrap();
        let feed = parse(BUNDLED).unwrap();
        let now = stamp(&feed.messages[0].starts_at).unwrap();
        let candidate = candidates(&feed, now).remove(0);
        assert!(db
            .insert_notification(&candidate, now, true, false)
            .unwrap()
            .is_none());
        assert!(db
            .insert_notification(&candidate, now + 1, true, true)
            .unwrap()
            .is_none());
        assert!(db.list_notifications().unwrap().is_empty());
        let db = Db::open_in_memory().unwrap();
        assert!(db
            .insert_notification(&candidate, now, false, true)
            .unwrap()
            .is_some());
        assert_eq!(db.list_notifications().unwrap()[0].delivery, "inbox_only");
    }

    #[test]
    fn only_a_fetched_feed_can_raise_popups() {
        let dir = tempfile::tempdir().unwrap();
        let cache = MarketCache::new(dir.path().to_owned());
        assert!(load(&cache).is_some_and(|feed| !feed.messages.is_empty()));
        assert!(fetched(&cache).is_none());
        cache.adopt_announcements(BUNDLED);
        assert!(fetched(&cache).is_some());
    }

    #[test]
    fn a_valid_withdrawal_replaces_bundled_messages_and_invalid_data_keeps_it() {
        let dir = tempfile::tempdir().unwrap();
        let cache = MarketCache::new(dir.path().to_owned());
        let empty = r#"{"version":1,"updated_at":"2026-10-12T00:00:00Z","messages":[]}"#;
        cache.adopt_announcements(empty);
        assert!(load(&cache).unwrap().messages.is_empty());
        cache.adopt_announcements("{broken");
        cache.adopt_announcements(BUNDLED);
        assert!(load(&cache).unwrap().messages.is_empty());
    }
}
