//! Curated, expiring messages. Remote prose is data, never an instruction to trade.
use super::{market::MarketCache, notifications};
use crate::notification_contract::{AnnouncementNotification, Candidate, NotificationContent};
use chrono::{DateTime, SecondsFormat};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};
use ts_rs::TS;

pub const EVENT: &str = "announcements-changed";
const BUNDLED: &str = include_str!("../../../../frontend/public/announcements.json");

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
pub struct AnnouncementFeed {
    pub version: u32,
    pub updated_at: String,
    pub messages: Vec<Announcement>,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
pub struct Announcement {
    pub id: String,
    pub starts_at: String,
    pub ends_at: String,
    pub title: String,
    pub body: String,
    pub source_url: String,
}

#[derive(Deserialize)]
struct Limits {
    max_bytes: usize,
    max_messages: usize,
    max_id: usize,
    max_title: usize,
    max_body: usize,
    max_window_seconds: i64,
    source_hosts: Vec<String>,
}

fn stamp(raw: &str) -> Option<i64> {
    if raw.len() != 20 { return None; }
    let value = DateTime::parse_from_rfc3339(raw).ok()?;
    (value.to_utc().to_rfc3339_opts(SecondsFormat::Secs, true) == raw).then(|| value.timestamp())
}

fn text(raw: &str, max: usize) -> bool {
    !raw.trim().is_empty() && raw.chars().count() <= max
        && !raw.chars().any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
}

fn permitted_source(raw: &str, limits: &Limits) -> bool {
    let Ok(source) = reqwest::Url::parse(raw) else { return false; };
    source.scheme() == "https" && source.username().is_empty() && source.password().is_none() && source.port().is_none()
        && source.host_str().is_some_and(|host| limits.source_hosts.iter().any(|allowed| allowed == host))
}

pub fn source_allowed(raw: &str) -> bool {
    let Ok(limits) = serde_json::from_str::<Limits>(include_str!("../../../../tests/fixtures/announcements/limits.json")) else { return false; };
    permitted_source(raw, &limits)
}

pub fn parse(raw: &str) -> Option<AnnouncementFeed> {
    let limits: Limits = serde_json::from_str(include_str!("../../../../tests/fixtures/announcements/limits.json")).ok()?;
    if raw.len() > limits.max_bytes { return None; }
    let feed: AnnouncementFeed = serde_json::from_str(raw).ok()?;
    if feed.version != 1 || stamp(&feed.updated_at).is_none() || feed.messages.len() > limits.max_messages { return None; }
    let mut ids = std::collections::BTreeSet::new();
    for message in &feed.messages {
        if !text(&message.id, limits.max_id)
            || !message.id.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'_' | b'-'))
            || !ids.insert(&message.id) || !text(&message.title, limits.max_title) || !text(&message.body, limits.max_body) { return None; }
        let window = stamp(&message.ends_at)?.checked_sub(stamp(&message.starts_at)?)?;
        if !(1..=limits.max_window_seconds).contains(&window) { return None; }
        if !permitted_source(&message.source_url, &limits) { return None; }
    }
    Some(feed)
}

pub fn validated_stamp(raw: &str) -> Option<String> {
    parse(raw).map(|feed| feed.updated_at)
}

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
    feed.messages.iter().filter_map(|message| {
        let start = stamp(&message.starts_at)?;
        let end = stamp(&message.ends_at)?;
        if now < start || now >= end { return None; }
        let mut candidate = Candidate::once(format!("announcement:{}", message.id), "announcements", message.title.clone(), message.body.clone(), "sell", now);
        // Clearing history or moving a deadline must never repeat the same ID.
        candidate.expires_at = i64::MAX;
        candidate.content = Some(NotificationContent::Announcement(AnnouncementNotification { source_url: message.source_url.clone(), ends_at: message.ends_at.clone() }));
        Some(candidate)
    }).collect()
}

pub fn deliver(app: &AppHandle) {
    if let Some(feed) = load(&app.state::<MarketCache>()) {
        for candidate in candidates(&feed, notifications::now()) {
            match notifications::publish(app, candidate) {
                Ok(true) => break,
                Ok(false) => {},
                Err(error) => { eprintln!("tennoworth: announcement could not be recorded: {error}"); break; },
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
        let cases: serde_json::Value = serde_json::from_str(include_str!("../../../../tests/fixtures/announcements/cases.json")).unwrap();
        for case in cases.as_array().unwrap() {
            let feed = parse(&case["feed"].to_string());
            assert_eq!(feed.is_some(), case["valid"].as_bool().unwrap(), "{}", case["name"]);
            let ids: Vec<_> = feed.as_ref().map(|feed| candidates(feed, stamp(case["now"].as_str().unwrap()).unwrap()).into_iter().map(|c| c.key.strip_prefix("announcement:").unwrap().to_string()).collect()).unwrap_or_default();
            assert_eq!(serde_json::to_value(ids).unwrap(), case["active_ids"], "{}", case["name"]);
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
        assert!(db.insert_notification(&candidate, now, true, true).unwrap().is_some());
        assert_eq!(db.list_notifications().unwrap()[0].delivery, "pending");
        db.clear_notifications().unwrap();
        db.prune_notifications(now + 365 * 86400).unwrap();
        drop(db);
        let db = Db::open(&path).unwrap();
        assert!(db.insert_notification(&candidate, now + 60, true, true).unwrap().is_none());
        assert!(db.list_notifications().unwrap().is_empty());
    }

    #[test]
    fn disabled_announcements_are_checkpointed_without_a_popup_or_inbox_entry() {
        let db = Db::open_in_memory().unwrap();
        let feed = parse(BUNDLED).unwrap();
        let now = stamp(&feed.messages[0].starts_at).unwrap();
        let candidate = candidates(&feed, now).remove(0);
        assert!(db.insert_notification(&candidate, now, true, false).unwrap().is_none());
        assert!(db.insert_notification(&candidate, now + 1, true, true).unwrap().is_none());
        assert!(db.list_notifications().unwrap().is_empty());
        let db = Db::open_in_memory().unwrap();
        assert!(db.insert_notification(&candidate, now, false, true).unwrap().is_some());
        assert_eq!(db.list_notifications().unwrap()[0].delivery, "inbox_only");
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
