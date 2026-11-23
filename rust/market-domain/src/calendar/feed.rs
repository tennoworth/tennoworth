//! The trader feed: what is coming that moves prices, soonest first. Baro's
//! window and which of his stock you hold, Prime Vault rotations and which of
//! your sets they unvault, Darvo's deal, and DE's dated events with the fixed
//! rewards you hold. Finished rows are dropped: a calendar that shows last
//! week's Baro teaches people to ignore it.
//!
//! An empty `affects` means two things, and `affects_known` says which: it
//! touches nothing you hold, or its reach cannot be told (no inventory, or a
//! reward list that is not complete).

use super::date::parse_ms;
use super::{
    contains_words, entries, field, filled, has_evidence, ms, positive, reward_rows, rows,
    set_parts, text, truthy_number, words, CalendarHolding, DAY, STALE_DAYS,
};
use serde::Serialize;
use serde_json::Value;
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum FeedKind {
    Baro,
    Vault,
    Deal,
    Event,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum FeedReach {
    /// No inventory yet: a scan would tell.
    Scan,
    None,
    Hits,
    /// Hits in a reward list that is not complete.
    PartialHits,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum FeedCoverage {
    Complete,
    Partial,
    Unknown,
}

/// The facts a row is worded from; the view words them in the reader's language.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum FeedText {
    Baro {
        r#where: Option<String>,
        stock: usize,
    },
    Vault {
        packs: usize,
    },
    Deal {
        item: String,
        discount: Option<f64>,
        price: Option<f64>,
    },
    Event {
        title: String,
        rewards: usize,
        credits: f64,
        coverage: FeedCoverage,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct FeedItem {
    /// Stable upstream id where one exists.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub id: Option<String>,
    pub kind: FeedKind,
    /// When it starts, as the snapshot gives it.
    pub at: String,
    /// When it ends, where there is an end.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub until: Option<String>,
    pub text: FeedText,
    /// Held slugs this row affects.
    pub affects: Vec<String>,
    /// False when the row's contents cannot be resolved to slugs, so an empty
    /// `affects` must not read as "this doesn't touch you".
    pub affects_known: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub reach: Option<FeedReach>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub stale: Option<bool>,
    /// Whole days since the row's surface was observed; absent when unknown.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub data_age_days: Option<f64>,
}

impl FeedItem {
    fn dated(kind: FeedKind, at: Option<&str>, until: Option<String>, text: FeedText) -> Self {
        FeedItem {
            id: None,
            kind,
            at: at.unwrap_or_default().to_owned(),
            until,
            text,
            affects: Vec::new(),
            affects_known: false,
            reach: None,
            stale: None,
            data_age_days: None,
        }
    }
}

/// Held slugs; a short list, so a scan is cheaper in the module than a set.
type Held<'a> = Option<Vec<&'a str>>;

/// Which held sets a Prime Vault rotation covers.
///
/// Rotation manifests are bundle SKUs with no market slug, so the only signal
/// is the name embedded in the path. A set matches when its name is a run of
/// whole words in a SKU - never a raw substring, which would flag a held
/// Bronco Prime on an Akbronco Prime pack - and no fuzzy scoring: telling
/// somebody their Nidus Prime is about to be unvaulted when it isn't is worse
/// than saying nothing. A set is held as the set or as any of its parts.
pub fn vault_affects(rotation: &Value, market: &Value, held: Option<&[&str]>) -> Vec<String> {
    let calendar = field(market, "calendar");
    let (Some(_), Some(held)) = (calendar.and_then(|c| field(c, "primes")), held) else {
        return Vec::new();
    };
    let skus: Vec<_> = rows(Some(rotation), "items")
        .iter()
        .map(|sku| words(sku.as_str().unwrap_or_default()))
        .collect();
    entries(calendar, "primes")
        .filter(|(set, prime)| {
            let Some(name) = filled(prime, "name") else {
                return false;
            };
            let needle = words(name);
            skus.iter().any(|sku| contains_words(sku, &needle))
                && (held.contains(&set.as_str())
                    || set_parts(market, set)
                        .iter()
                        .any(|part| text(part, "slug").is_some_and(|slug| held.contains(&slug))))
        })
        .map(|(set, _)| set.clone())
        .collect()
}

fn baro(market: &Value, held: &Held, out: &mut Vec<FeedItem>) {
    let Some(baro) = field(market, "baro") else {
        return;
    };
    let (Some(activation), Some(expiry)) = (filled(baro, "activation"), filled(baro, "expiry"))
    else {
        return;
    };
    let stock = rows(Some(baro), "inventory");
    let text = FeedText::Baro {
        r#where: filled(baro, "location").map(str::to_owned),
        stock: stock.len(),
    };
    let mut row = FeedItem::dated(
        FeedKind::Baro,
        Some(activation),
        Some(expiry.to_owned()),
        text,
    );
    // His stock is the stock whose price his arrival moves.
    row.affects = stock
        .iter()
        .filter_map(|line| filled(line, "slug"))
        .filter(|slug| held.as_ref().is_some_and(|h| h.contains(slug)))
        .map(str::to_owned)
        .collect();
    // Without an inventory we cannot say what it touches, which is not the
    // same as it touching nothing.
    row.affects_known = held.is_some() && !stock.is_empty();
    out.push(row);
}

fn vaults(market: &Value, held: &Held, out: &mut Vec<FeedItem>) {
    let primes_known = field(market, "calendar")
        .and_then(|c| field(c, "primes"))
        .is_some();
    for rotation in rows(field(market, "de"), "vault_rotation") {
        let text = FeedText::Vault {
            packs: rows(Some(rotation), "items").len(),
        };
        let until = text_owned(rotation, "expiry");
        let mut row = FeedItem::dated(
            FeedKind::Vault,
            super::text(rotation, "activation"),
            until,
            text,
        );
        row.affects = vault_affects(rotation, market, held.as_deref());
        row.affects_known = held.is_some() && primes_known;
        out.push(row);
    }
}

fn text_owned(value: &Value, key: &str) -> Option<String> {
    text(value, key).map(str::to_owned)
}

fn deals(market: &Value, out: &mut Vec<FeedItem>) {
    for deal in rows(field(market, "de"), "deals") {
        let (discount, price) = (
            truthy_number(deal, "discount"),
            truthy_number(deal, "sale_price"),
        );
        let priced = discount.is_some() && price.is_some();
        let text = FeedText::Deal {
            item: text(deal, "item").unwrap_or_default().to_owned(),
            discount: discount.filter(|_| priced),
            price: price.filter(|_| priced),
        };
        let mut row = FeedItem::dated(FeedKind::Deal, super::text(deal, "expiry"), None, text);
        // Darvo sells for platinum from DE's store and never touches a
        // player's tradeable holdings: "affects nothing" is a fact here.
        row.affects_known = true;
        out.push(row);
    }
}

fn events(market: &Value, held: &Held, now: f64, out: &mut Vec<FeedItem>) {
    let provenance = field(market, "surface_provenance");
    for event in reward_rows(market) {
        let source =
            provenance.and_then(|p| p.get(format!("world.{}s", super::printed(event, "source"))));
        // A failed read records the attempt as the data timestamp, so its age
        // would read as freshly updated. Its true age is unknown.
        let stamp = source
            .filter(|p| has_evidence(p.get("disposition")))
            .and_then(|p| filled(p, "data_fetched_at"));
        let data_age_days = stamp
            .and_then(parse_ms)
            .map(|observed| ((now - ms(observed)) / ms(DAY)).floor().max(0.0) + 0.0);
        let mut slugs: Vec<&str> = Vec::new();
        let (mut rewards, mut credits) = (0, 0.0);
        for group in rows(Some(event), "groups") {
            credits += positive(group, "credits").unwrap_or(0.0);
            for reward in rows(Some(group), "rewards") {
                rewards += 1;
                if let Some(slug) = filled(reward, "slug") {
                    if !slugs.contains(&slug) {
                        slugs.push(slug);
                    }
                }
            }
        }
        let affects: Vec<String> = match held {
            Some(held) => slugs
                .into_iter()
                .filter(|s| held.contains(s))
                .map(str::to_owned)
                .collect(),
            None => Vec::new(),
        };
        let completeness = text(event, "completeness");
        let (complete, unknown) = (
            completeness == Some("complete"),
            completeness == Some("unknown"),
        );
        let reach = if unknown {
            FeedReach::Unknown
        } else if held.is_none() {
            FeedReach::Scan
        } else if !affects.is_empty() {
            if complete {
                FeedReach::Hits
            } else {
                FeedReach::PartialHits
            }
        } else if complete {
            FeedReach::None
        } else {
            FeedReach::Unknown
        };
        let coverage = if unknown {
            FeedCoverage::Unknown
        } else if complete {
            FeedCoverage::Complete
        } else {
            FeedCoverage::Partial
        };
        let text = FeedText::Event {
            title: text(event, "title").unwrap_or_default().to_owned(),
            rewards,
            credits,
            coverage,
        };
        let mut row = FeedItem::dated(
            FeedKind::Event,
            super::text(event, "starts_at"),
            text_owned(event, "ends_at"),
            text,
        );
        row.id = text_owned(event, "id");
        row.affects = affects;
        row.affects_known = matches!(reach, FeedReach::Hits | FeedReach::None);
        row.reach = Some(reach);
        row.stale = Some(data_age_days.is_none_or(|days| days >= STALE_DAYS));
        row.data_age_days = data_age_days;
        out.push(row);
    }
}

/// Everything dated that has not finished by `now`, soonest first.
pub fn build(market: &Value, owned: Option<&[CalendarHolding]>, now: f64) -> Vec<FeedItem> {
    let held: Held = owned.map(|owned| owned.iter().map(|h| h.slug.as_str()).collect());
    let mut out = Vec::new();
    baro(market, &held, &mut out);
    vaults(market, &held, &mut out);
    deals(market, &mut out);
    if field(market, "event_rewards").is_some() {
        events(market, &held, now, &mut out);
    }
    out.retain(|row| {
        let end = row
            .until
            .as_deref()
            .filter(|u| !u.is_empty())
            .unwrap_or(&row.at);
        parse_ms(end).is_some_and(|end| ms(end) > now)
    });
    // A start the page could not read sorts after every readable one; the
    // page's comparator answered NaN for it, which orders nothing.
    let start = |row: &FeedItem| parse_ms(&row.at).map_or(f64::INFINITY, ms);
    // An insertion sort: stable, and a handful of rows does not need more.
    for i in 1..out.len() {
        let mut j = i;
        while j > 0
            && out
                .get(j - 1)
                .zip(out.get(j))
                .is_some_and(|(a, b)| start(a) > start(b))
        {
            out.swap(j - 1, j);
            j -= 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::super::CalendarRequest;
    use super::*;
    use serde_json::{json, Value};

    const NOW: f64 = 1_787_356_800_000.0; // 2026-08-22T00:00:00Z

    fn feed(market: &Value, owned: Option<&[&str]>, now: f64) -> Vec<FeedItem> {
        let owned: Option<Vec<_>> =
            owned.map(|o| o.iter().map(|s| json!({ "slug": s, "count": 1 })).collect());
        let request: CalendarRequest =
            serde_json::from_value(json!({ "market": market, "owned": owned, "now": now }))
                .unwrap();
        build(&request.market, request.owned.as_deref(), request.now)
    }

    fn market() -> Value {
        json!({
            "baro": {
                "activation": "2026-08-25T13:00:00Z", "expiry": "2026-08-27T13:00:00Z", "location": "Pluto Relay",
                "inventory": [{ "item": "Primed Fury", "slug": "primed_fury", "ducats": 350 }, { "item": "Ki'Teer Sekhara", "ducats": 200 }],
            },
            "calendar": { "primes": {
                "revenant_prime_set": { "name": "Revenant Prime", "vaulted": true },
                "nidus_prime_set": { "name": "Nidus Prime", "vaulted": true },
                "volt_prime_set": { "name": "Volt Prime", "vaulted": false },
            } },
            "set_to_parts": { "nidus_prime_set": { "name": "Nidus Prime", "parts": [{ "slug": "nidus_prime_systems", "component_name": "Systems" }] } },
            "de": {
                "vault_rotation": [{
                    "activation": "2026-08-28T18:00:00Z", "expiry": "2026-09-25T18:00:00Z",
                    "items": [
                        "/Lotus/Types/StoreItems/Packages/MegaPrimeVault/MPVRevenantPrimeSinglePack",
                        "/Lotus/Types/StoreItems/Packages/MegaPrimeVault/MPVNidusPrimeSinglePack",
                    ],
                }],
                "deals": [{ "item": "Lex", "expiry": "2026-08-23T00:00:00Z", "discount": 40, "sale_price": 114 }],
            },
        })
    }

    fn event_market() -> Value {
        json!({
            "event_rewards": { "goals": {
                "complete": {
                    "id": "complete", "source": "goal", "title": "Community Goal",
                    "starts_at": "2026-08-24T00:00:00Z", "ends_at": "2026-08-30T00:00:00Z", "completeness": "complete",
                    "groups": [{ "kind": "final", "rewards": [{ "unique": "/Lotus/PrimedFury", "name": "Primed Fury", "slug": "primed_fury", "quantity": 1 }] }],
                },
                "partial": {
                    "id": "partial", "source": "goal", "title": "Milestone Goal",
                    "starts_at": "2026-08-25T00:00:00Z", "ends_at": "2026-08-31T00:00:00Z", "completeness": "partial",
                    "groups": [{ "kind": "milestone", "threshold": 100, "credits": 50000, "rewards": [
                        { "unique": "/Lotus/RevenantPrimeSet", "name": "Revenant Prime Set", "slug": "revenant_prime_set", "quantity": 1 },
                        { "unique": "/Lotus/Unknown", "name": "Unknown Reward", "quantity": 1 },
                    ] }],
                },
            } },
            "surface_provenance": { "world.goals": {
                "disposition": "published_fresh", "attempted_at": "2026-08-22T00:00:00Z", "data_fetched_at": "2026-08-22T00:00:00Z",
            } },
        })
    }

    fn titled<'a>(rows: &'a [FeedItem], title: &str) -> &'a FeedItem {
        rows.iter()
            .find(|r| matches!(&r.text, FeedText::Event { title: t, .. } if t == title))
            .unwrap()
    }

    fn vault_hits(market: &Value, rotation: &Value, held: &[&str]) -> Vec<String> {
        vault_affects(rotation, market, Some(held))
    }

    #[test]
    fn vault_rotations_match_held_sets_and_parts_by_whole_words() {
        let m = market();
        let rotation = &m["de"]["vault_rotation"][0];
        assert_eq!(
            vault_hits(&m, rotation, &["revenant_prime_set"]),
            ["revenant_prime_set"]
        );
        assert_eq!(
            vault_hits(&m, rotation, &["nidus_prime_systems"]),
            ["nidus_prime_set"]
        );
        assert!(vault_hits(&m, rotation, &["volt_prime_set"]).is_empty());

        let substrings = json!({ "calendar": { "primes": {
            "bronco_prime_set": { "name": "Bronco Prime", "vaulted": true },
            "akbronco_prime_set": { "name": "Akbronco Prime", "vaulted": true },
            "bo_prime_set": { "name": "Bo Prime", "vaulted": true },
            "limbo_prime_set": { "name": "Limbo Prime", "vaulted": true },
        } } });
        let pack = json!({ "activation": "2026-08-28T18:00:00Z", "items": [
            "/Lotus/Types/StoreItems/Packages/MegaPrimeVault/MPVAkbroncoPrimeSinglePack",
            "/Lotus/Types/StoreItems/Packages/MegaPrimeVault/MPVLimboPrimeSinglePack",
        ] });
        assert_eq!(
            vault_hits(
                &substrings,
                &pack,
                &["bronco_prime_set", "bo_prime_set", "akbronco_prime_set"]
            ),
            ["akbronco_prime_set"]
        );

        let ampersand = json!({ "calendar": { "primes": { "cobra_and_crane_prime_set": { "name": "Cobra & Crane Prime", "vaulted": true } } } });
        let pack = json!({ "items": ["/Lotus/Types/StoreItems/Packages/MegaPrimeVault/MPVCobraAndCranePrimeSinglePack"] });
        assert_eq!(
            vault_hits(&ampersand, &pack, &["cobra_and_crane_prime_set"]),
            ["cobra_and_crane_prime_set"]
        );

        assert!(vault_affects(rotation, &m, None).is_empty());
        assert!(vault_hits(
            &json!({ "items": {} }),
            &m["de"]["vault_rotation"][0],
            &["x"]
        )
        .is_empty());
    }

    #[test]
    fn vault_matching_agrees_with_the_desktop_reminder() {
        let cases: Value = serde_json::from_str(include_str!(
            "../../../../tests/fixtures/notifications/vault.json"
        ))
        .unwrap();
        for case in cases.as_array().unwrap() {
            let held: Vec<&str> = case["held"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap())
                .collect();
            let mut hits = vault_hits(&case["market"], &case["rotation"], &held);
            hits.sort();
            assert_eq!(json!(hits), case["expected"], "{}", case["name"]);
        }
    }

    #[test]
    fn orders_soonest_first_and_drops_what_has_finished() {
        let kinds: Vec<_> = feed(&market(), Some(&[]), NOW)
            .into_iter()
            .map(|r| r.kind)
            .collect();
        assert_eq!(kinds, [FeedKind::Deal, FeedKind::Baro, FeedKind::Vault]);
        // A calendar showing last week's Baro teaches people to ignore it.
        assert!(feed(&market(), Some(&[]), 1_790_726_400_000.0).is_empty());
        // Baro stays until his window closes, not until it opens.
        let mid_visit = feed(&market(), Some(&[]), 1_787_702_400_000.0);
        assert!(mid_visit.iter().any(|r| r.kind == FeedKind::Baro));
    }

    #[test]
    fn baro_marks_held_stock_and_says_when_it_cannot_tell() {
        let rows = feed(&market(), Some(&["primed_fury"]), NOW);
        let baro = rows.iter().find(|r| r.kind == FeedKind::Baro).unwrap();
        assert_eq!(baro.affects, ["primed_fury"]);
        assert!(baro.affects_known);

        let rows = feed(&market(), None, NOW);
        let baro = rows.iter().find(|r| r.kind == FeedKind::Baro).unwrap();
        assert!(baro.affects.is_empty());
        assert!(!baro.affects_known);
        assert!(
            rows.iter()
                .find(|r| r.kind == FeedKind::Deal)
                .unwrap()
                .affects_known
        );
    }

    #[test]
    fn is_empty_on_a_snapshot_with_none_of_this() {
        assert!(feed(&json!({ "items": {} }), None, NOW).is_empty());
        assert!(build(&Value::Null, None, NOW).is_empty());
    }

    #[test]
    fn events_carry_their_dates_ids_and_credits_and_ask_for_a_scan() {
        let rows = feed(&event_market(), None, NOW);
        let summary: Vec<_> = rows
            .iter()
            .map(|r| (r.at.as_str(), r.until.as_deref(), r.reach))
            .collect();
        assert_eq!(
            summary,
            [
                (
                    "2026-08-24T00:00:00Z",
                    Some("2026-08-30T00:00:00Z"),
                    Some(FeedReach::Scan)
                ),
                (
                    "2026-08-25T00:00:00Z",
                    Some("2026-08-31T00:00:00Z"),
                    Some(FeedReach::Scan)
                ),
            ]
        );
        let rows = feed(&event_market(), Some(&[]), NOW);
        let milestone = titled(&rows, "Milestone Goal");
        assert_eq!(milestone.id.as_deref(), Some("partial"));
        assert!(
            matches!(milestone.text, FeedText::Event { credits, coverage: FeedCoverage::Partial, .. } if credits == 50_000.0)
        );
    }

    #[test]
    fn reach_never_turns_partial_coverage_into_reassurance() {
        let mut m = event_market();
        m["event_rewards"]["goals"]["unknown"] = json!({
            "id": "unknown", "source": "goal", "title": "Jobs Goal",
            "starts_at": "2026-08-26T00:00:00Z", "ends_at": "2026-09-01T00:00:00Z", "completeness": "unknown", "groups": [],
        });
        let rows = feed(&m, Some(&[]), NOW);
        let unknown = rows
            .iter()
            .find(|r| r.id.as_deref() == Some("unknown"))
            .unwrap();
        assert_eq!(unknown.reach, Some(FeedReach::Unknown));
        assert!(!unknown.affects_known);
        assert!(matches!(
            unknown.text,
            FeedText::Event {
                coverage: FeedCoverage::Unknown,
                ..
            }
        ));

        let miss = feed(&event_market(), Some(&["something_else"]), NOW);
        assert_eq!(titled(&miss, "Community Goal").reach, Some(FeedReach::None));
        assert_eq!(
            titled(&miss, "Milestone Goal").reach,
            Some(FeedReach::Unknown)
        );
        let hit = feed(&event_market(), Some(&["revenant_prime_set"]), NOW);
        assert_eq!(
            titled(&hit, "Milestone Goal").reach,
            Some(FeedReach::PartialHits)
        );
        assert!(!titled(&hit, "Milestone Goal").affects_known);
    }

    fn with_event_surface(disposition: Value, fetched: &str) -> Value {
        let mut m = event_market();
        let mut event = m["event_rewards"]["goals"]["complete"].clone();
        event["id"] = json!("event");
        event["source"] = json!("event");
        event["title"] = json!("Event Reward");
        m["event_rewards"]["events"] = json!({ "event": event });
        m["surface_provenance"]["world.events"] = json!({ "disposition": disposition, "attempted_at": "2026-08-22T00:00:00Z", "data_fetched_at": fetched });
        m
    }

    #[test]
    fn each_source_is_measured_by_its_own_stamp() {
        let rows = feed(
            &with_event_surface(json!("merged_partial"), "2026-08-01T00:00:00Z"),
            Some(&[]),
            NOW,
        );
        assert_eq!(titled(&rows, "Community Goal").stale, Some(false));
        assert_eq!(titled(&rows, "Event Reward").stale, Some(true));
        assert_eq!(titled(&rows, "Event Reward").data_age_days, Some(21.0));

        let fresh = feed(
            &with_event_surface(json!("published_fresh"), "2026-08-22T00:00:00Z"),
            Some(&[]),
            NOW,
        );
        assert_eq!(titled(&fresh, "Event Reward").stale, Some(false));
    }

    #[test]
    fn a_stamp_that_records_a_failed_read_has_no_age() {
        for disposition in [
            "empty_invalid",
            "empty_unavailable",
            "empty_unchanged",
            "preserved_invalid",
            "preserved_unavailable",
        ] {
            let rows = feed(
                &with_event_surface(json!(disposition), "2026-08-22T00:00:00Z"),
                Some(&[]),
                NOW,
            );
            let event = titled(&rows, "Event Reward");
            assert_eq!(event.stale, Some(true), "{disposition}");
            assert_eq!(event.data_age_days, None, "{disposition}");
        }
        // A null disposition is not an observation; a missing one is a legacy row that is.
        let rows = feed(
            &with_event_surface(Value::Null, "2026-08-22T00:00:00Z"),
            Some(&[]),
            NOW,
        );
        assert_eq!(titled(&rows, "Event Reward").data_age_days, None);
        let mut legacy = with_event_surface(Value::Null, "2026-08-22T00:00:00Z");
        legacy["surface_provenance"]["world.events"]
            .as_object_mut()
            .unwrap()
            .remove("disposition");
        let rows = feed(&legacy, Some(&[]), NOW);
        assert_eq!(titled(&rows, "Event Reward").data_age_days, Some(0.0));
    }

    #[test]
    fn turns_stale_at_the_age_the_pipeline_warns_at() {
        assert_eq!(STALE_DAYS, 7.0);
        let stale = |fetched: &str| {
            let rows = feed(
                &with_event_surface(json!("merged_partial"), fetched),
                Some(&[]),
                NOW,
            );
            titled(&rows, "Event Reward").stale
        };
        assert_eq!(stale("2026-08-16T00:00:00Z"), Some(false));
        assert_eq!(stale("2026-08-15T00:00:00Z"), Some(true));
    }
}
