//! Clock-driven reminders use the same cached snapshot as the desktop, never a
//! second upstream feed. A visit's carried stock is not evidence of current stock.
use crate::{
    persistence::Db,
    services::market::{self, MarketCache},
    services::notifications::{self, Candidate},
    services::sellables::{self, MarketData},
};
use chrono::{DateTime, Local, Timelike};
use serde_json::Value;
use std::collections::BTreeSet;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

pub const MARKET_EVENT: &str = "market-refreshed";
const DAY: i64 = 86400;
fn string<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or("")
}
fn rows(v: &Value) -> impl Iterator<Item = &Value> {
    v.as_array().into_iter().flatten()
}
fn field<'a>(v: &'a Value, key: &str) -> &'a Value {
    v.get(key).unwrap_or(&Value::Null)
}
fn stamp(s: &str) -> Option<i64> {
    DateTime::parse_from_rfc3339(s).ok().map(|d| d.timestamp())
}
fn fresh(s: &str, now: i64) -> bool {
    stamp(s).is_some_and(|t| (0..=DAY).contains(&now.saturating_sub(t)))
}
fn surface_fresh(market: &Value, key: &str, now: i64) -> bool {
    let provenance = market.get("surface_provenance").and_then(|p| p.get(key));
    let source_stamp = provenance
        .and_then(|p| p.get("data_fetched_at"))
        .and_then(Value::as_str)
        .or_else(|| {
            market
                .get("surface_fetched_at")
                .and_then(|p| p.get(key))
                .and_then(Value::as_str)
        });
    source_stamp.is_some_and(|s| fresh(s, now))
}

/// Only the current phase is eligible after a restart; earlier phases are not queued.
fn phase(start: i64, end: i64, now: i64, before: bool) -> Option<(i64, &'static str)> {
    if start >= end || now >= end {
        None
    } else if now >= start && now >= end - 3600 {
        Some((3, "ends within an hour"))
    } else if now >= start {
        Some((2, "is here"))
    } else if before && now >= start - 3600 {
        Some((1, "arrives within an hour"))
    } else {
        None
    }
}

fn words(text: &str) -> Vec<String> {
    let text = text.replace('&', " And ");
    let chars: Vec<char> = text.chars().collect();
    let mut spaced = String::new();
    for (i, c) in chars.iter().enumerate() {
        let previous = i.checked_sub(1).and_then(|i| chars.get(i));
        let next = chars.get(i + 1);
        if c.is_ascii_uppercase()
            && previous.is_some_and(|p| {
                p.is_ascii_lowercase()
                    || p.is_ascii_digit()
                    || (p.is_ascii_uppercase() && next.is_some_and(char::is_ascii_lowercase))
            })
        {
            spaced.push(' ');
        }
        spaced.push(c.to_ascii_lowercase());
    }
    spaced
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}
pub(crate) fn vault_affects(
    rotation: &Value,
    market: &Value,
    held: &BTreeSet<String>,
) -> Vec<String> {
    let Some(primes) = market
        .get("calendar")
        .and_then(|c| c.get("primes"))
        .and_then(Value::as_object)
    else {
        return vec![];
    };
    let skus: Vec<_> = rows(field(rotation, "items"))
        .filter_map(Value::as_str)
        .map(words)
        .collect();
    primes
        .iter()
        .filter_map(|(slug, prime)| {
            let needle = words(string(prime, "name"));
            if needle.is_empty()
                || !skus
                    .iter()
                    .any(|sku| sku.windows(needle.len()).any(|w| w == needle))
            {
                return None;
            }
            let parts = market
                .get("set_to_parts")
                .and_then(|p| p.get(slug))
                .and_then(|p| p.get("parts"))
                .unwrap_or(&Value::Null);
            (held.contains(slug) || rows(parts).any(|p| held.contains(string(p, "slug"))))
                .then(|| slug.clone())
        })
        .collect()
}

pub fn scheduled(market: &Value, held: &BTreeSet<String>, now: i64) -> Vec<Candidate> {
    let mut out = vec![];
    if let Some(baro) = market.get("baro") {
        if let (Some(start), Some(end)) = (
            stamp(string(baro, "activation")),
            stamp(string(baro, "expiry")),
        ) {
            if let Some((stage, label)) = phase(start, end, now, true) {
                let current_stock = field(baro, "inventory").is_array()
                    && surface_fresh(market, "baro", now)
                    && stamp(string(baro, "inventory_for")) == Some(start);
                let mut body = format!(
                    "{} · Published schedule: {} to {}.",
                    string(baro, "location"),
                    string(baro, "activation"),
                    string(baro, "expiry")
                );
                if current_stock {
                    let stock: Vec<_> = rows(field(baro, "inventory")).collect();
                    let matches: Vec<_> = stock
                        .iter()
                        .filter(|s| held.contains(string(s, "slug")))
                        .map(|s| string(s, "item"))
                        .collect();
                    body.push_str(&format!(" {} stock items.", stock.len()));
                    if !matches.is_empty() {
                        body.push_str(&format!(" You hold: {}.", matches.join(", ")));
                    }
                } else {
                    body.push_str(" Current stock is not yet verified.");
                }
                body.push_str(" Open Baro for stock and the ducat planner.");
                let mut candidate = Candidate::once(
                    format!("baro:{start}"),
                    "baro",
                    format!("Baro Ki'Teer {label}"),
                    body,
                    "baro",
                    now,
                );
                candidate.stage = stage;
                candidate.expires_at = end + DAY;
                out.push(candidate);
            }
        }
    }
    if surface_fresh(market, "world.vault_rotation", now) {
        for rotation in rows(
            market
                .get("de")
                .and_then(|d| d.get("vault_rotation"))
                .unwrap_or(&Value::Null),
        ) {
            let hits = vault_affects(rotation, market, held);
            if !hits.is_empty() {
                if let Some(c) = calendar_candidate(
                    "vault",
                    "Prime Vault rotation",
                    string(rotation, "activation"),
                    string(rotation, "expiry"),
                    &hits,
                    false,
                    now,
                ) {
                    out.push(c);
                }
            }
        }
    }
    if let Some(events) = market.get("event_rewards") {
        for child in ["goals", "events"] {
            if !surface_fresh(market, &format!("world.{child}"), now) {
                continue;
            }
            for event in events
                .get(child)
                .and_then(Value::as_object)
                .into_iter()
                .flat_map(|o| o.values())
            {
                let completeness = string(event, "completeness");
                if !["complete", "partial"].contains(&completeness) {
                    continue;
                }
                let hits: BTreeSet<_> = rows(field(event, "groups"))
                    .flat_map(|g| rows(field(g, "rewards")))
                    .map(|r| string(r, "slug"))
                    .filter(|s| held.contains(*s))
                    .map(str::to_string)
                    .collect();
                if hits.is_empty() {
                    continue;
                }
                if let Some(c) = calendar_candidate(
                    &format!("{child}:{}", string(event, "id")),
                    string(event, "title"),
                    string(event, "starts_at"),
                    string(event, "ends_at"),
                    &hits.into_iter().collect::<Vec<_>>(),
                    completeness != "complete",
                    now,
                ) {
                    out.push(c);
                }
            }
        }
    }
    out
}
fn calendar_candidate(
    id: &str,
    title: &str,
    start: &str,
    end: &str,
    hits: &[String],
    partial: bool,
    now: i64,
) -> Option<Candidate> {
    let (start_time, end_time) = (stamp(start)?, stamp(end)?);
    let (stage, label) = phase(start_time, end_time, now, false)?;
    let mut c = Candidate::once(
        format!("calendar:{id}:{start_time}:{end_time}"),
        "calendar",
        format!("{title} {}", if stage == 2 { "is active" } else { label }),
        format!(
            "Affects your holdings: {}.{} Ends {end}.",
            hits.join(", "),
            if partial {
                " Reward coverage is partial."
            } else {
                ""
            }
        ),
        "routines",
        now,
    );
    c.stage = stage;
    c.expires_at = end_time + DAY;
    Some(c)
}

pub fn digest(
    market: &Value,
    rows: &[sellables::SellableRow],
    inventory_at: &str,
    now: i64,
    local_day: &str,
    local_hour: u32,
) -> Option<Candidate> {
    if local_hour < 18
        || rows.is_empty()
        || !fresh(string(market, "updated_at"), now)
        || !fresh(inventory_at, now)
    {
        return None;
    }
    let opportunities: Vec<_> = rows
        .iter()
        .take(5)
        .map(|r| format!("{} ×{}: ~{:.0}p each", r.name, r.sellable_qty, r.price))
        .collect();
    Some(Candidate::once(format!("digest:{local_day}"), "digest", "Today's sell opportunities".into(),
        format!("{}. Estimated market values, not guaranteed sales. Inventory: {inventory_at}. Prices: {}.", opportunities.join(" · "), string(market, "updated_at")), "sell", now))
}

fn evaluate(app: &AppHandle) {
    let db = app.state::<Db>();
    let cache = app.state::<MarketCache>();
    let Some(raw) = cache
        .cached()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .or_else(|| serde_json::from_str(sellables::BUNDLED_MARKET).ok())
    else {
        return;
    };
    // Refuse a malformed snapshot before joining inventory against a bundled fallback.
    if !raw.get("items").is_some_and(Value::is_object) {
        return;
    }
    let market = MarketData::load(&cache);
    let now = notifications::now();
    let snapshots = db.list_snapshots(1).unwrap_or_default();
    let inventory_at = snapshots.first().map(|s| s.taken_at.as_str()).unwrap_or("");
    let held = if fresh(inventory_at, now) {
        market
            .overlay_owned(&db)
            .unwrap_or_default()
            .into_iter()
            .filter(|(_, count)| *count > 0)
            .map(|(slug, _)| slug)
            .collect()
    } else {
        BTreeSet::new()
    };
    for candidate in scheduled(&raw, &held, now) {
        notifications::send(app, candidate);
    }
    let local = Local::now();
    if let Some(candidate) = digest(
        &raw,
        &sellables::rank_sellables(&db, &market),
        inventory_at,
        now,
        &local.format("%Y-%m-%d").to_string(),
        local.hour(),
    ) {
        notifications::send(app, candidate);
    }
    if let Err(e) = db.prune_notifications(now) {
        eprintln!("tennoworth: notification cleanup failed: {e}");
    }
}
pub fn start(app: AppHandle) {
    if let Err(e) = std::thread::Builder::new()
        .name("notification-reminders".into())
        .spawn(move || {
            let mut refreshed: Option<Instant> = None;
            loop {
                if refreshed.is_none_or(|t| t.elapsed() >= Duration::from_secs(15 * 60)) {
                    let result = market::refresh(&app.state::<MarketCache>().dir());
                    if result.updated {
                        crate::shell::tray::rebuild_tray(&app);
                        let _ = app.emit(MARKET_EVENT, ());
                    }
                    refreshed = Some(Instant::now());
                }
                evaluate(&app);
                std::thread::sleep(Duration::from_secs(60));
            }
        })
    {
        eprintln!("tennoworth: reminders could not start: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    const START: &str = "2026-09-04T13:00:00Z";
    const END: &str = "2026-09-06T13:00:00Z";
    fn market() -> Value {
        json!({"baro": {"activation": START, "expiry": END, "location": "Pluto Relay", "inventory_for": START, "inventory": [{"item":"Primed Flow", "slug":"primed_flow"}]}, "surface_provenance":{"baro":{"data_fetched_at": START}}})
    }
    #[test]
    fn phase_boundaries_and_resume_only_choose_the_current_phase() {
        let start = stamp(START).unwrap();
        let end = stamp(END).unwrap();
        assert_eq!(phase(start, end, start - 3601, true), None);
        assert_eq!(phase(start, end, start - 3600, true).unwrap().0, 1);
        assert_eq!(phase(start, end, start, true).unwrap().0, 2);
        assert_eq!(phase(start, end, end - 3600, true).unwrap().0, 3);
        assert_eq!(phase(start, end, end, true), None);
        assert_eq!(phase(start, start, start, true), None);
        assert_eq!(phase(start, end, start - 1, false), None);
        assert_eq!(phase(start, end, start + 3600, false).unwrap().0, 2);
    }
    #[test]
    fn stale_or_previous_visit_stock_never_personalizes_a_schedule() {
        let now = stamp(START).unwrap();
        let held = BTreeSet::from(["primed_flow".into()]);
        let mut m = market();
        assert!(scheduled(&m, &held, now)[0]
            .body
            .contains("You hold: Primed Flow"));
        m["baro"]["inventory_for"] = json!("2026-08-21T13:00:00Z");
        assert!(!scheduled(&m, &held, now)[0].body.contains("You hold"));
        assert!(scheduled(&m, &held, now)[0]
            .body
            .contains("not yet verified"));
        m["baro"]["inventory_for"] = json!(START);
        assert!(!scheduled(&m, &held, now + DAY + 1)[0]
            .body
            .contains("You hold"));
        m["baro"]["activation"] = json!("invalid");
        assert!(scheduled(&m, &held, now).is_empty());
    }
    #[test]
    fn calendar_requires_fresh_known_matches_and_keeps_partial_coverage() {
        let now = stamp(START).unwrap();
        let mut m = json!({"surface_provenance":{"world.goals":{"data_fetched_at": START}},"event_rewards":{"goals":{"g":{"id":"g","title":"Event","starts_at":START,"ends_at":END,"completeness":"partial","groups":[{"rewards":[{"slug":"primed_flow"}]}]}}}});
        let held = BTreeSet::from(["primed_flow".into()]);
        let out = scheduled(&m, &held, now);
        assert_eq!(out.len(), 1);
        assert!(out[0].body.contains("partial"));
        assert!(scheduled(&m, &BTreeSet::new(), now).is_empty());
        assert!(scheduled(&m, &held, now + DAY + 1).is_empty());
        m["event_rewards"]["goals"]["g"]["completeness"] = json!("unknown");
        assert!(scheduled(&m, &held, now).is_empty());
    }
    #[test]
    fn digest_requires_fresh_inventory_prices_and_local_evening() {
        let now = stamp(START).unwrap();
        let m = json!({"updated_at": START});
        let rows = vec![sellables::SellableRow {
            name: "Part".into(),
            slug: "part".into(),
            sellable_qty: 2,
            price: 12.0,
            score: 3.0,
        }];
        assert!(digest(&m, &rows, START, now, "2026-09-04", 17).is_none());
        let c = digest(&m, &rows, START, now, "2026-09-04", 18).unwrap();
        assert_eq!(c.key, "digest:2026-09-04");
        assert!(c.body.contains("Part ×2: ~12p each"));
        assert!(digest(&m, &[], START, now, "2026-09-04", 18).is_none());
        assert!(digest(&m, &rows, "", now, "2026-09-04", 18).is_none());
        assert!(digest(&m, &rows, START, now + DAY + 1, "2026-09-05", 18).is_none());
        assert!(!fresh(START, now - 1));
    }
    #[test]
    fn event_relevance_matches_the_frontend_fixture() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../../tests/fixtures/notifications/events.json"
        ))
        .unwrap();
        let held = fixture["held"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        let actual: Vec<_> = scheduled(
            &fixture["market"],
            &held,
            stamp(fixture["now"].as_str().unwrap()).unwrap(),
        )
        .into_iter()
        .map(|c| c.title.strip_suffix(" is active").unwrap().to_string())
        .collect();
        assert_eq!(serde_json::to_value(actual).unwrap(), fixture["expected"]);
    }

    #[test]
    fn vault_matching_agrees_with_the_frontend() {
        let cases: Value = serde_json::from_str(include_str!(
            "../../../../tests/fixtures/notifications/vault.json"
        ))
        .unwrap();
        for c in cases.as_array().unwrap() {
            let held = c["held"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap().to_string())
                .collect();
            let mut actual = vault_affects(&c["rotation"], &c["market"], &held);
            actual.sort();
            assert_eq!(
                serde_json::to_value(actual).unwrap(),
                c["expected"],
                "{}",
                c["name"]
            );
        }
    }
}
