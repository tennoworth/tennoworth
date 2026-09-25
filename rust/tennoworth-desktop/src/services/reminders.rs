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
/// Whether a surface holds evidence observed recently enough to draw a
/// conclusion from.
///
/// A recent stamp is necessary but not sufficient. Several dispositions record
/// the *attempt* as the data timestamp - `empty_invalid` and its siblings mean
/// the read was tried and produced nothing usable, and `reconcile` stamps both
/// fields with that attempt. Checking the stamp alone therefore reports a failed
/// read as current, and a caller goes on to say something about a surface it
/// never observed.
///
/// Only a disposition that represents an actual observation counts. A surface
/// with no `disposition` at all is treated by its stamp: older snapshots predate
/// the field, and their stamp is a genuine fetch time.
fn surface_fresh(market: &Value, key: &str, now: i64) -> bool {
    let provenance = market.get("surface_provenance").and_then(|p| p.get(key));
    if let Some(disposition) = provenance
        .and_then(|p| p.get("disposition"))
        .and_then(Value::as_str)
    {
        // `preserved_unchanged` is not here on purpose: its payload was
        // revalidated by content hash, so it is current despite its older stamp.
        // The stale-age question that exemption answers is a different one from
        // "is there evidence to act on", and this caller only asks the latter.
        if !matches!(
            disposition,
            "published_fresh" | "merged_partial" | "cleared_authoritative_empty"
        ) {
            return false;
        }
    }
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
/// Start the reminder loop.
///
/// `on_market_refresh` is the presentation work a refreshed market implies -
/// today, rebuilding the tray so it stops offering prices that were just
/// replaced. It is injected by the composition root rather than called here
/// because the tray is the shell's, and a service that names the shell layer
/// cannot be used without it.
pub fn start(app: AppHandle, on_market_refresh: impl Fn(&AppHandle) + Send + 'static) {
    if let Err(e) = std::thread::Builder::new()
        .name("notification-reminders".into())
        .spawn(move || {
            let mut refreshed: Option<Instant> = None;
            loop {
                if refreshed.is_none_or(|t| t.elapsed() >= Duration::from_secs(15 * 60)) {
                    let result = market::refresh(&app.state::<MarketCache>().dir());
                    if result.updated {
                        on_market_refresh(&app);
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
    fn freshness_dispositions_follow_shared_consumer_cases() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../../tests/fixtures/surface-freshness/cases.json"
        ))
        .unwrap();
        let now = stamp(fixture["now"].as_str().unwrap()).unwrap();
        for case in fixture["cases"].as_array().unwrap() {
            let mut market = json!({"surface_fetched_at": {"baro": case["data_fetched_at"]}});
            if !case["disposition"].is_null() {
                market["surface_provenance"] = json!({"baro": {
                    "disposition": case["disposition"],
                    "data_fetched_at": case["data_fetched_at"]
                }});
            }
            assert_eq!(
                surface_fresh(&market, "baro", now),
                case["reminder_fresh"].as_bool().unwrap(),
                "{}",
                case["name"].as_str().unwrap()
            );
        }
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
    /// A surface can carry a stamp from *this attempt* without the attempt
    /// having observed anything. `empty_invalid` means the read was attempted
    /// and produced nothing usable, and `reconcile` records both timestamps as
    /// that attempt - so a stamp-only freshness check calls a failed read
    /// current and lets the caller draw conclusions from nothing.
    ///
    /// This is the shipped shape: `world.events` is `empty_invalid` with
    /// `data_fetched_at == attempted_at`.
    #[test]
    fn a_failed_read_is_not_fresh_however_recent_its_attempt() {
        let now = stamp(START).unwrap();
        let attempted = START;
        let stale_evidence = "2026-08-01T00:00:00Z";

        // The attempt is a minute old, but nothing was observed.
        let empty_invalid = json!({"surface_provenance": {"world.events": {
            "disposition": "empty_invalid",
            "attempted_at": attempted,
            "data_fetched_at": attempted,
        }}});
        assert!(
            !surface_fresh(&empty_invalid, "world.events", now),
            "an unobserved surface is not current evidence"
        );

        // Every preserved/empty state is the same: the stamp is the attempt,
        // not an observation.
        for disposition in [
            "preserved_unavailable",
            "preserved_invalid",
            "empty_unavailable",
            "empty_unchanged",
            "empty_invalid",
        ] {
            let m = json!({"surface_provenance": {"world.events": {
                "disposition": disposition,
                "attempted_at": attempted,
                "data_fetched_at": attempted,
            }}});
            assert!(
                !surface_fresh(&m, "world.events", now),
                "{disposition} reports a failed read as fresh"
            );
        }

        // A preserved surface that genuinely still holds older evidence is not
        // fresh either - its stamp is the older one and the window catches it.
        let kept = json!({"surface_provenance": {"world.events": {
            "disposition": "preserved_unavailable",
            "attempted_at": attempted,
            "data_fetched_at": stale_evidence,
        }}});
        assert!(!surface_fresh(&kept, "world.events", now));

        // And a real observation is still fresh, so the rule cannot pass by
        // refusing everything.
        for disposition in ["published_fresh", "merged_partial"] {
            let m = json!({"surface_provenance": {"world.events": {
                "disposition": disposition,
                "attempted_at": attempted,
                "data_fetched_at": attempted,
            }}});
            assert!(
                surface_fresh(&m, "world.events", now),
                "{disposition} is observed evidence and must stay fresh"
            );
        }
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
