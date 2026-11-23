//! The homepage's hand-off sample: three real market rows scored the way the
//! desktop scores a scan, with owned counts the page labels as sample values.
//! The pipeline picks and scores them so the site shows the native priority
//! instead of carrying its own copy of the formula.

use crate::pipeline::render::{ItemEntry, Snapshot};
use market_domain::math::sell_priority::{score_row_weighted, PricedEntry};
use market_domain::scoring::{UsageIndex, UsageSet};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HandoffPick {
    pub slug: String,
    /// A sample count, not anyone's inventory.
    pub owned: u32,
    /// The desktop's sell priority for `owned` copies, rounded.
    pub score: i64,
}

/// The sample counts, in pick order.
const OWNED: [u32; 3] = [3, 1, 2];
/// A row worth showing trades at least this often (48 h) and this high.
const LIQUID_VOL: i64 = 10;
const LIQUID_AVG: f64 = 10.0;
/// How deep each candidate list is read before falling back.
const VAULTED_DEPTH: usize = 24;
const RISER_DEPTH: usize = 40;

fn liquid(entry: &ItemEntry) -> bool {
    entry.vol >= LIQUID_VOL && entry.avg >= LIQUID_AVG
}

/// Today's median against the 90-day baseline, in percent.
fn delta_pct(entry: &ItemEntry) -> Option<f64> {
    (entry.median_90d > 0.0)
        .then(|| (entry.median_now - entry.median_90d) / entry.median_90d * 100.0)
}

/// Best first by `key`, ties by slug so a rebuild picks the same rows.
fn ranked<'a>(
    rows: impl Iterator<Item = (&'a String, &'a ItemEntry)>,
    key: impl Fn(&ItemEntry) -> f64,
) -> Vec<(&'a String, &'a ItemEntry)> {
    let mut rows: Vec<_> = rows.collect();
    rows.sort_by(|a, b| key(b.1).total_cmp(&key(a.1)).then_with(|| a.0.cmp(b.0)));
    rows
}

/// Up to `limit` more liquid rows not already picked, until the sample is full.
fn take<'a>(
    picks: &mut Vec<(&'a String, &'a ItemEntry)>,
    rows: impl IntoIterator<Item = (&'a String, &'a ItemEntry)>,
    limit: usize,
) {
    let mut taken = 0;
    for (slug, entry) in rows {
        if picks.len() >= OWNED.len() || taken >= limit {
            return;
        }
        if liquid(entry) && !picks.iter().any(|(picked, _)| *picked == slug) {
            picks.push((slug, entry));
            taken += 1;
        }
    }
}

/// One vaulted set if there is one, then the most valuable risers, then the
/// most traded rows - the first three liquid ones, each slug once.
pub fn sample(snapshot: &Snapshot) -> Vec<HandoffPick> {
    let items = &snapshot.items;
    let vaulted = ranked(
        snapshot
            .vault_status
            .iter()
            .filter(|(_, status)| status.as_str() == "vaulted")
            .filter_map(|(slug, _)| items.get_key_value(slug))
            .filter(|(_, e)| e.avg > 0.0),
        |e| e.avg,
    );
    let risers = ranked(
        ranked(
            items
                .iter()
                .filter(|(_, e)| liquid(e))
                .filter(|(_, e)| delta_pct(e).is_some_and(|d| d > 0.0)),
            |e| delta_pct(e).unwrap_or(0.0),
        )
        .into_iter()
        .take(RISER_DEPTH),
        |e| e.avg,
    );
    let traded = ranked(items.iter(), |e| e.vol as f64);

    let mut picks = Vec::new();
    take(&mut picks, vaulted.into_iter().take(VAULTED_DEPTH), 1);
    take(&mut picks, risers, OWNED.len());
    take(&mut picks, traded, OWNED.len());

    let usage: BTreeMap<String, serde_json::Value> = snapshot
        .usage
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let sets: BTreeMap<String, UsageSet> = snapshot
        .set_to_parts
        .iter()
        .filter_map(|(k, v)| Some((k.clone(), serde_json::from_value(v.clone()).ok()?)))
        .collect();
    let index = UsageIndex::new(&usage, &sets);
    picks
        .into_iter()
        .zip(OWNED)
        .map(|((slug, entry), owned)| {
            let priced = PricedEntry {
                vol: entry.vol as f64,
                low_sell: entry.low_sell,
                avg: entry.avg,
                median_now: entry.median_now,
                median_90d: entry.median_90d,
            };
            let share = index.resolve(slug).0.map(|u| u.share);
            let score = score_row_weighted(f64::from(owned), &priced, share).sell_score;
            HandoffPick {
                slug: slug.clone(),
                owned,
                score: score.round() as i64,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::render::{assemble_snapshot, UsageHistorySurface};
    use serde_json::json;
    use std::collections::HashMap;

    fn item(avg: f64, vol: i64, median_now: f64, median_90d: f64) -> ItemEntry {
        let row: crate::pipeline::csv_input::CsvRow = Default::default();
        let mut entry = crate::pipeline::render::render_item(&row, &Default::default());
        entry.avg = avg;
        entry.low_sell = avg;
        entry.vol = vol;
        entry.median_now = median_now;
        entry.median_90d = median_90d;
        entry
    }

    fn snapshot(
        items: &[(&str, ItemEntry)],
        vault: &[(&str, &str)],
        usage: HashMap<String, serde_json::Value>,
        sets: HashMap<String, serde_json::Value>,
    ) -> Snapshot {
        assemble_snapshot(
            jiff::Timestamp::UNIX_EPOCH,
            HashMap::new(),
            items
                .iter()
                .map(|(s, e)| (s.to_string(), e.clone()))
                .collect(),
            HashMap::new(),
            sets,
            HashMap::new(),
            vault
                .iter()
                .map(|(s, v)| (s.to_string(), v.to_string()))
                .collect(),
            HashMap::new(),
            HashMap::new(),
            HashMap::new(),
            HashMap::new(),
            HashMap::new(),
            usage,
            UsageHistorySurface::default(),
            HashMap::new(),
        )
    }

    fn slugs(picks: &[HandoffPick]) -> Vec<&str> {
        picks.iter().map(|p| p.slug.as_str()).collect()
    }

    #[test]
    fn picks_a_vaulted_row_then_the_most_valuable_risers() {
        let snap = snapshot(
            &[
                ("vaulted_cheap", item(20.0, 30, 20.0, 20.0)),
                ("vaulted_rich", item(200.0, 30, 200.0, 200.0)),
                ("riser_small", item(30.0, 30, 40.0, 20.0)),
                ("riser_big", item(90.0, 30, 100.0, 80.0)),
                ("faller", item(500.0, 30, 400.0, 500.0)),
            ],
            &[("vaulted_cheap", "vaulted"), ("vaulted_rich", "vaulted")],
            HashMap::new(),
            HashMap::new(),
        );
        let picks = sample(&snap);
        assert_eq!(slugs(&picks), ["vaulted_rich", "riser_big", "riser_small"]);
        assert_eq!(picks.iter().map(|p| p.owned).collect::<Vec<_>>(), OWNED);
    }

    #[test]
    fn skips_thin_rows_and_falls_back_to_the_most_traded() {
        let snap = snapshot(
            &[
                ("thin_vaulted", item(300.0, 2, 300.0, 300.0)),
                ("cheap_riser", item(5.0, 50, 6.0, 3.0)),
                ("busy", item(40.0, 90, 40.0, 40.0)),
                ("busier", item(40.0, 120, 40.0, 40.0)),
            ],
            &[("thin_vaulted", "vaulted")],
            HashMap::new(),
            HashMap::new(),
        );
        assert_eq!(slugs(&sample(&snap)), ["busier", "busy"]);
    }

    #[test]
    fn scores_with_the_desktop_formula_and_inherited_usage() {
        let usage_row = |share: f64| json!({ "name": "Set", "category": "Primary", "year": 2025, "share": share, "peak_mr": 10, "by_mr": [1] });
        let snap = snapshot(
            &[("part", item(100.0, 10, 100.0, 100.0))],
            &[],
            HashMap::from([("set".to_string(), usage_row(1.2))]),
            HashMap::from([("set".to_string(), json!({ "parts": [{ "slug": "part" }] }))]),
        );
        let picks = sample(&snap);
        let priced = PricedEntry {
            vol: 10.0,
            low_sell: 100.0,
            avg: 100.0,
            median_now: 100.0,
            median_90d: 100.0,
        };
        let expected = score_row_weighted(3.0, &priced, Some(1.2))
            .sell_score
            .round() as i64;
        assert_eq!(
            picks,
            [HandoffPick {
                slug: "part".into(),
                owned: 3,
                score: expected
            }]
        );
        assert_ne!(
            expected,
            score_row_weighted(3.0, &priced, None).sell_score.round() as i64
        );
    }

    #[test]
    fn breaks_ties_by_slug_and_answers_an_empty_market() {
        let rows = [
            ("b", item(50.0, 30, 50.0, 50.0)),
            ("a", item(50.0, 30, 50.0, 50.0)),
            ("c", item(50.0, 30, 50.0, 50.0)),
        ];
        let snap = snapshot(&rows, &[], HashMap::new(), HashMap::new());
        assert_eq!(slugs(&sample(&snap)), ["a", "b", "c"]);
        assert!(sample(&snapshot(&[], &[], HashMap::new(), HashMap::new())).is_empty());
    }
}
