//! Plain-text, expiring announcement feed shared by publishers and readers.
use chrono::{DateTime, SecondsFormat};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
pub struct AnnouncementFeed {
    pub version: u32,
    pub updated_at: String,
    pub messages: Vec<Announcement>,
    /// Official dates for the calendar. Optional so readers that predate it
    /// (desktop 0.8.103) keep accepting the feed: they ignore the field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub events: Option<Vec<OfficialEvent>>,
}

/// A dated announcement from DE that worldState does not carry, such as a
/// devstream or an update, entered by the maintainer with its source.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
pub struct OfficialEvent {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub starts_at: String,
    /// Absent for a single moment such as a stream start.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub ends_at: Option<String>,
    pub source_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub note: Option<String>,
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
    max_events: usize,
    max_event_note: usize,
    max_event_window_seconds: i64,
    event_kinds: Vec<String>,
}

fn valid_id(raw: &str, limits: &Limits) -> bool {
    text(raw, limits.max_id)
        && raw
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'_' | b'-'))
}

fn valid_event(event: &OfficialEvent, limits: &Limits) -> bool {
    let Some(start) = stamp(&event.starts_at) else {
        return false;
    };
    let window_ok = match &event.ends_at {
        None => true,
        Some(end) => stamp(end)
            .and_then(|end| end.checked_sub(start))
            .is_some_and(|window| (1..=limits.max_event_window_seconds).contains(&window)),
    };
    valid_id(&event.id, limits)
        && limits.event_kinds.iter().any(|kind| kind == &event.kind)
        && text(&event.title, limits.max_title)
        && window_ok
        && event
            .note
            .as_deref()
            .is_none_or(|note| text(note, limits.max_event_note))
        && permitted_source(&event.source_url, limits)
}

pub fn stamp(raw: &str) -> Option<i64> {
    if raw.len() != 20 {
        return None;
    }
    let value = DateTime::parse_from_rfc3339(raw).ok()?;
    (value.to_utc().to_rfc3339_opts(SecondsFormat::Secs, true) == raw).then(|| value.timestamp())
}

// Bidirectional controls can make a title or deadline read differently from its stored text.
fn text(raw: &str, max: usize) -> bool {
    !raw.trim().is_empty()
        && raw.chars().count() <= max
        && !raw.chars().any(|c| {
            (c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
                || matches!(
                    c,
                    '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}'
                )
        })
}

fn permitted_source(raw: &str, limits: &Limits) -> bool {
    let Ok(source) = url::Url::parse(raw) else {
        return false;
    };
    source.scheme() == "https"
        && source.username().is_empty()
        && source.password().is_none()
        && source.port().is_none()
        && source
            .host_str()
            .is_some_and(|host| limits.source_hosts.iter().any(|allowed| allowed == host))
}

pub fn source_allowed(raw: &str) -> bool {
    let Ok(limits) = serde_json::from_str::<Limits>(include_str!(
        "../../../tests/fixtures/announcements/limits.json"
    )) else {
        return false;
    };
    permitted_source(raw, &limits)
}

pub fn parse(raw: &str) -> Option<AnnouncementFeed> {
    let limits: Limits = serde_json::from_str(include_str!(
        "../../../tests/fixtures/announcements/limits.json"
    ))
    .ok()?;
    if raw.len() > limits.max_bytes {
        return None;
    }
    let feed: AnnouncementFeed = serde_json::from_str(raw).ok()?;
    if feed.version != 1
        || stamp(&feed.updated_at).is_none()
        || feed.messages.len() > limits.max_messages
    {
        return None;
    }
    let mut ids = std::collections::BTreeSet::new();
    for message in &feed.messages {
        if !valid_id(&message.id, &limits)
            || !ids.insert(&message.id)
            || !text(&message.title, limits.max_title)
            || !text(&message.body, limits.max_body)
        {
            return None;
        }
        let window = stamp(&message.ends_at)?.checked_sub(stamp(&message.starts_at)?)?;
        if !(1..=limits.max_window_seconds).contains(&window) {
            return None;
        }
        if !permitted_source(&message.source_url, &limits) {
            return None;
        }
    }
    if let Some(events) = &feed.events {
        let mut ids = std::collections::BTreeSet::new();
        if events.len() > limits.max_events
            || !events
                .iter()
                .all(|event| ids.insert(&event.id) && valid_event(event, &limits))
        {
            return None;
        }
    }
    Some(feed)
}

pub fn validated_stamp(raw: &str) -> Option<String> {
    parse(raw).map(|feed| feed.updated_at)
}

#[cfg(test)]
#[test]
fn shared_feed_cases_preserve_validity_and_active_windows() {
    let cases: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/announcements/cases.json"
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
        let now = stamp(case["now"].as_str().unwrap()).unwrap();
        let ids: Vec<_> = feed
            .map(|feed| {
                feed.messages
                    .into_iter()
                    .filter(|m| {
                        now >= stamp(&m.starts_at).unwrap() && now < stamp(&m.ends_at).unwrap()
                    })
                    .map(|m| m.id)
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
}
