//! The market browser's lists over the snapshot: the name index, search,
//! movers, the vaulted list, the disposition log, the hand-off sample and the
//! bargains. Each list is one call: the page sends the rows a list reads, in
//! snapshot order, and gets back positions into them (plus the numbers only
//! the rule computes), so the rows it renders stay the snapshot's own objects.
//! Inputs are read as the page's code read them: a field "as a number" is the
//! JSON number or `null` when it is not one; `Number(x)` fields arrive with
//! `NaN` as `null`.

use crate::math::collate::{locale_cmp, sorted};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::cmp::Ordering;
use std::collections::{BTreeMap, HashSet};
use ts_rs::TS;

/// The catalog stores display names lowercased (it is the WFM name to slug
/// join key). A readable label upper-cases each word's first character, with
/// JavaScript's ASCII-only `\b\w`: a letter after a non-ASCII one starts a word.
pub fn title_case(name: &str) -> String {
    let mut prev_word = false;
    name.chars()
        .map(|c| {
            let word = c.is_ascii_alphanumeric() || c == '_';
            let out = if word && !prev_word {
                c.to_ascii_uppercase()
            } else {
                c
            };
            prev_word = word;
            out
        })
        .collect()
}

/// The label of a slug the catalog does not name.
pub fn slug_name(slug: &str) -> String {
    title_case(&slug.replace('_', " "))
}

/// Move vs the 90-day baseline, in percent. `None` without a positive
/// baseline (dividing by it is junk - pre-split snapshots and brand-new items
/// land here) or without a current median.
pub fn delta_pct(now: Option<f64>, base: Option<f64>) -> Option<f64> {
    let base = base.filter(|b| *b > 0.0)?;
    Some((now? - base) / base * 100.0)
}

#[derive(Debug, Clone, Deserialize, TS)]
pub struct BrowseIndexRequest {
    /// `catalog` entries in snapshot order: lowercased name, slug.
    pub catalog: Option<Vec<(String, String)>>,
    /// `items` slugs in snapshot order, each with whether its entry is present.
    pub items: Option<Vec<(String, bool)>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct BrowseName {
    pub slug: String,
    pub name: String,
    pub name_lower: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
pub struct BrowseNames {
    /// One entry per priceable catalog name, for type-ahead search. Search is a
    /// sell tool: an un-priceable quest key is noise.
    pub names: Vec<BrowseName>,
    /// The label of every named catalog slug and every item slug.
    pub labels: Vec<(String, String)>,
}

pub fn browse_index(request: BrowseIndexRequest) -> BrowseNames {
    let priced: Option<HashSet<&str>> = request.items.as_ref().map(|items| {
        items
            .iter()
            .filter(|(_, present)| *present)
            .map(|(slug, _)| slug.as_str())
            .collect()
    });
    let mut names = Vec::new();
    let mut labels = BTreeMap::new();
    for (name_lower, slug) in request.catalog.iter().flatten() {
        if priced.as_ref().is_some_and(|p| !p.contains(slug.as_str())) {
            continue;
        }
        let name = title_case(name_lower);
        labels.insert(slug.clone(), name.clone());
        names.push(BrowseName {
            slug: slug.clone(),
            name,
            name_lower: name_lower.clone(),
        });
    }
    for (slug, _) in request.items.iter().flatten() {
        if !labels.contains_key(slug) {
            labels.insert(slug.clone(), slug_name(slug));
        }
    }
    BrowseNames {
        names,
        labels: labels.into_iter().collect(),
    }
}

/// Descending `b - a` order. A `NaN` reading, which the page's comparator
/// left to the engine, sorts last.
fn desc(a: Option<f64>, b: Option<f64>) -> Ordering {
    let read = |v: Option<f64>| v.unwrap_or(f64::NEG_INFINITY);
    read(b).partial_cmp(&read(a)).unwrap_or(Ordering::Equal)
}

#[derive(Debug, Clone, Deserialize, TS)]
pub struct SearchRequest {
    /// The query as the page's `trim().toLowerCase()` left it.
    pub query: String,
    pub limit: usize,
    /// The index names that have a market entry: lowercased name, and `vol`
    /// as `Number(vol)`.
    pub names: Vec<(String, Option<f64>)>,
}

/// Prefix matches above mid-word matches, each by volume, so "primed"
/// surfaces the "Primed ..." mods before "... Primed ..." parts.
pub fn search(request: &SearchRequest) -> Vec<usize> {
    if request.query.is_empty() {
        return Vec::new();
    }
    let vol = |i: &usize| request.names.get(*i).and_then(|n| n.1);
    let by_vol = |a: &usize, b: &usize| desc(vol(a), vol(b));
    let (mut starts, mut contains) = (Vec::new(), Vec::new());
    for (i, (name, _)) in request.names.iter().enumerate() {
        match name.find(&request.query) {
            Some(0) => starts.push(i),
            Some(_) => contains.push(i),
            None => {}
        }
    }
    let mut found = sorted(starts, &by_vol);
    found.extend(sorted(contains, &by_vol));
    found.truncate(request.limit);
    found
}

#[derive(Debug, Clone, Copy, Deserialize, TS)]
pub struct MoverItem {
    /// As a number.
    pub avg: Option<f64>,
    /// As `Number(vol)`.
    pub vol: Option<f64>,
    pub median_now: Option<f64>,
    pub median_90d: Option<f64>,
}

#[derive(Debug, Clone, Deserialize, TS)]
pub struct MoversRequest {
    /// The present `items` entries, in snapshot order.
    pub items: Vec<MoverItem>,
    /// Defaults to 20 trades / 48 h.
    #[serde(default)]
    #[ts(optional)]
    pub min_vol: Option<f64>,
    /// Defaults to 10p.
    #[serde(default)]
    #[ts(optional)]
    pub min_price: Option<f64>,
    /// Defaults to 8 a side.
    #[serde(default)]
    #[ts(optional)]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
pub struct Movers {
    pub risers: Vec<usize>,
    pub fallers: Vec<usize>,
}

/// Top risers and fallers by the move vs the 90-day median. The volume floor
/// drops thin books (a 200% "move" on 3 trades is noise), and the price floor
/// drops cheap junk (a 100% swing on a 3p relic is not worth plat).
pub fn top_movers(request: &MoversRequest) -> Movers {
    let min_vol = request.min_vol.unwrap_or(20.0);
    let min_price = request.min_price.unwrap_or(10.0);
    let limit = request.limit.unwrap_or(8);
    let mut rows: Vec<(usize, f64)> = Vec::new();
    for (i, e) in request.items.iter().enumerate() {
        if e.vol.is_some_and(|v| v < min_vol) {
            continue;
        }
        if e.avg.is_none_or(|avg| avg < min_price) {
            continue;
        }
        match delta_pct(e.median_now, e.median_90d) {
            Some(d) if d != 0.0 => rows.push((i, d)),
            _ => {}
        }
    }
    let side = |keep: fn(f64) -> bool, order: fn(f64, f64) -> Ordering| {
        let side: Vec<(usize, f64)> = rows.iter().copied().filter(|r| keep(r.1)).collect();
        sorted(side, &|a, b| order(a.1, b.1))
            .into_iter()
            .take(limit)
            .map(|r| r.0)
            .collect()
    };
    Movers {
        risers: side(
            |d| d > 0.0,
            |a, b| b.partial_cmp(&a).unwrap_or(Ordering::Equal),
        ),
        fallers: side(
            |d| d < 0.0,
            |a, b| a.partial_cmp(&b).unwrap_or(Ordering::Equal),
        ),
    }
}

#[derive(Debug, Clone, Deserialize, TS)]
pub struct VaultedRow {
    /// The `vault_status` value when it is a string.
    pub status: Option<String>,
    /// The entry's `avg` as a number; `null` without an entry.
    pub avg: Option<f64>,
}

#[derive(Debug, Clone, Deserialize, TS)]
pub struct VaultedRequest {
    /// `vault_status` in snapshot order.
    pub rows: Vec<VaultedRow>,
    /// Defaults to 12.
    #[serde(default)]
    #[ts(optional)]
    pub limit: Option<usize>,
}

/// The highest-value currently vaulted items. Only `vaulted`: vaulting-soon
/// and available are different signals.
pub fn vaulted_top(request: &VaultedRequest) -> Vec<usize> {
    let rows: Vec<(usize, f64)> = request
        .rows
        .iter()
        .enumerate()
        .filter(|(_, r)| r.status.as_deref() == Some("vaulted"))
        .filter_map(|(i, r)| r.avg.filter(|avg| *avg > 0.0).map(|avg| (i, avg)))
        .collect();
    sorted(rows, &|a, b| desc(Some(a.1), Some(b.1)))
        .into_iter()
        .take(request.limit.unwrap_or(12))
        .map(|r| r.0)
        .collect()
}

#[derive(Debug, Clone, Deserialize, TS)]
pub struct DispositionRequest {
    /// `rivens.changes` as published.
    #[ts(type = "unknown")]
    pub changes: Value,
    /// Defaults to 12.
    #[serde(default)]
    #[ts(optional)]
    pub limit: Option<usize>,
}

/// Disposition changes from the rolling log, newest first, capped. Raises and
/// cuts are both kept - DE says it only raises now, but if a cut ever ships,
/// hiding it would be the wrong call. Rows without a slug or finite values drop.
pub fn disposition_changes(request: &DispositionRequest) -> Result<Vec<usize>, String> {
    let Some(changes) = request.changes.as_array() else {
        return Ok(Vec::new());
    };
    let mut rows: Vec<(usize, &str, &str)> = Vec::new();
    for (i, change) in changes.iter().enumerate() {
        let Some(c) = change.as_object() else {
            continue;
        };
        let Some(slug) = c.get("slug").and_then(Value::as_str) else {
            continue;
        };
        if !["from", "to"]
            .iter()
            .all(|k| c.get(*k).is_some_and(Value::is_number))
        {
            continue;
        }
        let seen = match c.get("seen_at") {
            None | Some(Value::Null) => "",
            Some(Value::String(s)) => s,
            Some(_) => return Err("A disposition change's seen_at must be a string.".into()),
        };
        rows.push((i, seen, slug));
    }
    Ok(sorted(rows, &|a, b| {
        locale_cmp(b.1, a.1).then_with(|| locale_cmp(a.2, b.2))
    })
    .into_iter()
    .take(request.limit.unwrap_or(12))
    .map(|r| r.0)
    .collect())
}

#[derive(Debug, Clone, Deserialize, TS)]
pub struct HandoffCandidate {
    /// Absent when the pick has none, which the page reads as `undefined`.
    #[serde(default)]
    #[ts(type = "unknown", optional)]
    pub owned: Value,
    #[serde(default)]
    #[ts(type = "unknown", optional)]
    pub score: Value,
    /// The entry's `avg` as `Number(avg)`.
    pub avg: Option<f64>,
}

#[derive(Debug, Clone, Deserialize, TS)]
pub struct HandoffRequest {
    /// The published picks that name a present market entry, in order.
    pub picks: Vec<HandoffCandidate>,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
pub struct HandoffPick {
    pub at: usize,
    pub owned: f64,
    pub score: f64,
    /// `owned` x `avg`, rounded as `Math.round` does; `null` when `avg` is `NaN`.
    pub potential: Option<f64>,
}

/// `Math.round`: the nearest integer, halves toward +infinity.
fn js_round(x: f64) -> f64 {
    let floor = x.floor();
    if x - floor >= 0.5 {
        floor + 1.0
    } else {
        floor
    }
}

const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

/// A pick whose numbers are not a count and a score is left out rather than
/// shown as a plausible zero.
pub fn handoff_sample(request: &HandoffRequest) -> Vec<HandoffPick> {
    request
        .picks
        .iter()
        .enumerate()
        .filter_map(|(at, pick)| {
            let owned = pick
                .owned
                .as_f64()
                .filter(|o| o.fract() == 0.0 && o.abs() <= MAX_SAFE_INTEGER && *o > 0.0)?;
            let score = pick.score.as_f64().filter(|s| s.is_finite() && *s >= 0.0)?;
            let potential = pick
                .avg
                .map(|avg| js_round(owned * avg))
                .filter(|p| !p.is_nan());
            Some(HandoffPick {
                at,
                owned,
                score,
                potential,
            })
        })
        .collect()
}

// Two shapes of mispriced book in the snapshot, sized on 2026-11-17 (16 rows):
// - flip: a live bid at least BARGAIN_MIN_MARGIN over the cheapest ask, so the
//   item can be bought and sold straight to that buyer;
// - cheap: one ask at most CHEAP_AT of the 90-day median while the five
//   cheapest still average CHEAP_BOOK of it, so the rest of the book agrees
//   on the usual price and one seller is far under it.
// Thin items carry the fantasy bids and wrong-rank asks, so both need trades
// behind them, and a flip's bid must be believable against the median.
// A snapshot is hours old and these close fast: a row is a lead to check live,
// never a promise.
pub const BARGAIN_MIN_VOL: f64 = 10.0;
pub const BARGAIN_MIN_MARGIN: f64 = 3.0;
pub const FLIP_MAX_BID: f64 = 1.5;
pub const CHEAP_AT: f64 = 0.6;
pub const CHEAP_BOOK: f64 = 0.85;
pub const CHEAP_MIN_MEDIAN: f64 = 10.0;

/// One entry's book, each field read as `Number(x) || 0`.
#[derive(Debug, Clone, Copy, Deserialize, TS)]
pub struct BargainItem {
    pub ask: f64,
    pub bid: f64,
    pub median: f64,
    pub vol: f64,
    pub low5: f64,
}

#[derive(Debug, Clone, Deserialize, TS)]
pub struct BargainsRequest {
    /// Every `items` entry, in snapshot order.
    pub items: Vec<BargainItem>,
    /// Defaults to 12.
    #[serde(default)]
    #[ts(optional)]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum BargainKind {
    Flip,
    Cheap,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
pub struct BargainLead {
    pub at: usize,
    pub kind: BargainKind,
    pub ask: f64,
    /// flip: the bid that would buy it back; cheap: the 90-day median.
    pub resale: f64,
    pub margin: f64,
    pub vol: f64,
}

pub fn find_bargains(request: &BargainsRequest) -> Vec<BargainLead> {
    let mut out: Vec<BargainLead> = Vec::new();
    for (at, e) in request.items.iter().enumerate() {
        let (ask, median) = (e.ask, e.median);
        if ask <= 0.0 || median <= 0.0 || e.vol < BARGAIN_MIN_VOL {
            continue;
        }
        let lead = |kind, resale: f64| BargainLead {
            at,
            kind,
            ask,
            resale,
            margin: resale - ask,
            vol: e.vol,
        };
        if e.bid - ask >= BARGAIN_MIN_MARGIN && e.bid <= median * FLIP_MAX_BID {
            out.push(lead(BargainKind::Flip, e.bid));
        } else if median >= CHEAP_MIN_MEDIAN
            && ask <= median * CHEAP_AT
            && e.low5 >= median * CHEAP_BOOK
            && median - ask >= BARGAIN_MIN_MARGIN
        {
            out.push(lead(BargainKind::Cheap, median));
        }
    }
    let mut out = sorted(out, &|a, b| {
        desc(Some(a.margin), Some(b.margin)).then_with(|| desc(Some(a.vol), Some(b.vol)))
    });
    out.truncate(request.limit.unwrap_or(12));
    out
}

/// Whether the live book still offers what the snapshot showed: a flip needs
/// its margin over the live ask, a cheap ask its price.
pub fn bargain_holds(kind: BargainKind, seen_ask: f64, ask: f64, bid: Option<f64>) -> bool {
    match kind {
        BargainKind::Flip => bid.is_some_and(|bid| bid - ask >= BARGAIN_MIN_MARGIN),
        BargainKind::Cheap => ask <= seen_ask,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn title_case_follows_the_ascii_word_rule() {
        assert_eq!(title_case("mag prime set"), "Mag Prime Set");
        assert_eq!(
            title_case("mutalist alad v assassinate (key)"),
            "Mutalist Alad V Assassinate (Key)"
        );
        assert_eq!(title_case("höllvanian old town"), "HöLlvanian Old Town");
        assert_eq!(title_case("albrecht’s archive"), "Albrecht’S Archive");
        assert_eq!(slug_name("some_unknown_slug"), "Some Unknown Slug");
    }

    #[test]
    fn delta_needs_a_positive_baseline_and_a_current_median() {
        assert!((delta_pct(Some(60.0), Some(50.0)).unwrap() - 20.0).abs() < 1e-9);
        assert!((delta_pct(Some(30.0), Some(40.0)).unwrap() + 25.0).abs() < 1e-9);
        assert_eq!(delta_pct(Some(25.0), None), None);
        assert_eq!(delta_pct(Some(25.0), Some(0.0)), None);
        assert_eq!(delta_pct(Some(25.0), Some(-5.0)), None);
        assert_eq!(delta_pct(None, Some(40.0)), None);
    }

    fn index() -> BrowseNames {
        browse_index(BrowseIndexRequest {
            catalog: Some(vec![
                ("mag prime set".into(), "mag_prime_set".into()),
                ("quest key".into(), "quest_key".into()),
            ]),
            items: Some(vec![
                ("mag_prime_set".into(), true),
                ("loose_item".into(), true),
            ]),
        })
    }

    #[test]
    fn the_index_names_priced_catalog_entries_and_labels_every_item() {
        let idx = index();
        assert_eq!(
            idx.names,
            vec![BrowseName {
                slug: "mag_prime_set".into(),
                name: "Mag Prime Set".into(),
                name_lower: "mag prime set".into()
            }]
        );
        assert_eq!(
            idx.labels,
            vec![
                ("loose_item".into(), "Loose Item".into()),
                ("mag_prime_set".into(), "Mag Prime Set".into())
            ]
        );
        let empty = browse_index(BrowseIndexRequest {
            catalog: None,
            items: None,
        });
        assert!(empty.names.is_empty() && empty.labels.is_empty());
        let unpriced = browse_index(BrowseIndexRequest {
            catalog: Some(vec![("quest key".into(), "quest_key".into())]),
            items: None,
        });
        assert_eq!(unpriced.names.len(), 1);
    }

    #[test]
    fn search_ranks_prefixes_then_volume_and_caps() {
        let names = vec![
            ("mag prime set".to_string(), Some(50.0)),
            ("primed continuity".to_string(), Some(200.0)),
            ("primed flow".to_string(), Some(300.0)),
            ("arcane primed".to_string(), Some(900.0)),
        ];
        let found = |query: &str, limit| {
            search(&SearchRequest {
                query: query.into(),
                limit,
                names: names.clone(),
            })
        };
        assert_eq!(found("primed", 12), vec![2, 1, 3]);
        assert_eq!(found("primed", 1), vec![2]);
        assert_eq!(found("", 12), Vec::<usize>::new());
        assert_eq!(found("zzz", 12), Vec::<usize>::new());
    }

    fn mover(avg: f64, vol: f64, now: f64, base: f64) -> MoverItem {
        MoverItem {
            avg: Some(avg),
            vol: Some(vol),
            median_now: Some(now),
            median_90d: Some(base),
        }
    }

    #[test]
    fn movers_split_by_direction_behind_both_floors() {
        let items = vec![
            mover(120.0, 50.0, 60.0, 50.0),  // +20
            mover(40.0, 200.0, 30.0, 40.0),  // -25
            mover(35.0, 300.0, 44.0, 40.0),  // +10
            mover(500.0, 40.0, 90.0, 100.0), // -10
            mover(5.0, 3.0, 30.0, 10.0),     // thin
            mover(10.0, 100.0, 25.0, 25.0),  // flat
            mover(8.0, 80.0, 25.0, 0.0),     // no baseline
            mover(9.9, 100.0, 20.0, 10.0),   // under the price floor
            mover(10.0, 100.0, 10.5, 10.0),  // at the price floor
        ];
        let movers = |min_vol, limit| {
            top_movers(&MoversRequest {
                items: items.clone(),
                min_vol,
                min_price: None,
                limit,
            })
        };
        assert_eq!(
            movers(None, None),
            Movers {
                risers: vec![0, 2, 8],
                fallers: vec![1, 3]
            }
        );
        assert_eq!(movers(Some(1.0), None).risers, vec![0, 2, 8]);
        let cheap = top_movers(&MoversRequest {
            items: items.clone(),
            min_vol: Some(1.0),
            min_price: Some(1.0),
            limit: None,
        });
        assert_eq!(cheap.risers, vec![4, 7, 0, 2, 8]);
        assert_eq!(movers(Some(1.0), Some(1)).risers, vec![0]);
        let unread = MoverItem {
            vol: None,
            ..mover(120.0, 0.0, 60.0, 50.0)
        };
        assert_eq!(
            top_movers(&MoversRequest {
                items: vec![unread],
                min_vol: None,
                min_price: None,
                limit: None
            })
            .risers,
            vec![0]
        );
    }

    #[test]
    fn vaulted_keeps_priced_vaulted_rows_by_avg() {
        let row = |status: &str, avg: Option<f64>| VaultedRow {
            status: Some(status.into()),
            avg,
        };
        let rows = vec![
            row("vaulted", Some(120.0)),
            row("vaulted", Some(500.0)),
            row("vaulting-soon", Some(900.0)),
            row("available", Some(900.0)),
            row("vaulted", None),
            row("vaulted", Some(0.0)),
        ];
        assert_eq!(
            vaulted_top(&VaultedRequest {
                rows: rows.clone(),
                limit: None
            }),
            vec![1, 0]
        );
        assert_eq!(
            vaulted_top(&VaultedRequest {
                rows,
                limit: Some(1)
            }),
            vec![1]
        );
    }

    #[test]
    fn dispositions_are_newest_first_then_by_slug() {
        let changes = json!([
            { "slug": "braton", "from": 1.1, "to": 1.15, "seen_at": "2026-08-16T12:00:00Z" },
            { "slug": "lato", "from": 1.35, "to": 1.4, "seen_at": "2026-07-01T00:00:00Z" },
            { "slug": "kulstar", "from": 1.3, "to": 1.35, "seen_at": "2026-08-16T12:00:00Z" },
            { "slug": "bad", "from": "x", "to": 1, "seen_at": "2026-08-16T12:00:00Z" },
            { "slug": "unseen", "from": 1, "to": 1.1 },
            null
        ]);
        let run = |limit| {
            disposition_changes(&DispositionRequest {
                changes: changes.clone(),
                limit,
            })
        };
        assert_eq!(run(None).unwrap(), vec![0, 2, 1, 4]);
        assert_eq!(run(Some(1)).unwrap(), vec![0]);
        assert_eq!(
            disposition_changes(&DispositionRequest {
                changes: Value::Null,
                limit: None
            })
            .unwrap(),
            Vec::<usize>::new()
        );
    }

    #[test]
    fn handoff_keeps_counted_scored_picks_with_their_stack_value() {
        let pick =
            |owned: Value, score: Value, avg: Option<f64>| HandoffCandidate { owned, score, avg };
        let picks = vec![
            pick(json!(3), json!(75), Some(500.0)),
            pick(json!(0), json!(10), Some(500.0)),
            pick(json!(1.5), json!(10), Some(500.0)),
            pick(json!(1), Value::Null, Some(40.0)),
            pick(json!(1), json!(-1), Some(40.0)),
            pick(json!("2"), json!(10), Some(40.0)),
            pick(json!(2), json!(0), Some(2.25)),
            pick(json!(1), json!(5), None),
        ];
        let out = handoff_sample(&HandoffRequest { picks });
        assert_eq!(
            out.iter().map(|p| (p.at, p.potential)).collect::<Vec<_>>(),
            vec![(0, Some(1500.0)), (6, Some(5.0)), (7, None)]
        );
        let unread: HandoffRequest =
            serde_json::from_value(json!({ "picks": [{ "avg": 4.0 }] })).unwrap();
        assert!(handoff_sample(&unread).is_empty());
        assert_eq!(js_round(2.5), 3.0);
        assert_eq!(js_round(-2.5), -2.0);
        assert_eq!(js_round(0.49999999999999994), 0.0);
    }

    fn book(ask: f64, bid: f64, median: f64, vol: f64, low5: f64) -> BargainItem {
        BargainItem {
            ask,
            bid,
            median,
            vol,
            low5,
        }
    }

    #[test]
    fn bargains_find_flips_and_lone_cheap_asks() {
        let found =
            |items: Vec<BargainItem>, limit| find_bargains(&BargainsRequest { items, limit });
        assert_eq!(
            found(vec![book(30.0, 40.0, 49.0, 40.0, 0.0)], None),
            vec![BargainLead {
                at: 0,
                kind: BargainKind::Flip,
                ask: 30.0,
                resale: 40.0,
                margin: 10.0,
                vol: 40.0
            }]
        );
        assert_eq!(
            found(vec![book(30.0, 33.0, 49.0, 40.0, 0.0)], None)[0].margin,
            3.0
        );
        assert!(found(vec![book(999.0, 2000.0, 1000.0, 40.0, 0.0)], None).is_empty());
        let cheap = found(vec![book(14.0, 0.0, 24.0, 222.0, 23.6)], None);
        assert_eq!(
            (cheap[0].kind, cheap[0].resale, cheap[0].margin),
            (BargainKind::Cheap, 24.0, 10.0)
        );
        assert!(found(vec![book(14.0, 0.0, 24.0, 222.0, 15.0)], None).is_empty());
        assert!(found(vec![book(30.0, 40.0, 49.0, 9.0, 0.0)], None).is_empty());
        let ranked = found(
            vec![
                book(30.0, 34.0, 40.0, 20.0, 0.0),
                book(30.0, 40.0, 40.0, 20.0, 0.0),
                book(30.0, 36.0, 40.0, 20.0, 0.0),
            ],
            Some(2),
        );
        assert_eq!(ranked.iter().map(|b| b.at).collect::<Vec<_>>(), vec![1, 2]);
    }

    #[test]
    fn a_lead_holds_while_the_live_book_still_offers_it() {
        assert!(bargain_holds(BargainKind::Flip, 30.0, 31.0, Some(38.0)));
        assert!(!bargain_holds(BargainKind::Flip, 30.0, 36.0, Some(38.0)));
        assert!(!bargain_holds(BargainKind::Flip, 30.0, 31.0, None));
        assert!(bargain_holds(BargainKind::Cheap, 14.0, 14.0, None));
        assert!(!bargain_holds(BargainKind::Cheap, 14.0, 22.0, None));
    }
}
