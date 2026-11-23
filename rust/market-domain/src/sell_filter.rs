//! The Sell view's filter cascade: price / owned / type / kept-level / tag
//! chips, and the clauses only a preset sets (vault-only, ducats-only,
//! min-volume, min-median, types-any, spares, advice). The table's rows, the
//! tag chips' counts and the empty-state diagnosis are three ways of combining
//! the same clauses, so each clause exists once, here.
//!
//! The page sends each owned record already joined to its market entry and
//! its `score_inventory` fact, with numbers as `Number(x)` reads them
//! ([`JsNumber`]): the clauses compare them as the views did, including a
//! missing field failing every comparison.

use crate::advisor::Advice;
use crate::js::{self, JsNumber};
use crate::math::sell_priority;
use crate::scoring::Timing;
use crate::tags::{self, TagGroup};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use ts_rs::TS;

#[derive(Debug, Clone, Deserialize, TS)]
pub struct SellFilters {
    pub min_price: JsNumber,
    pub min_owned: JsNumber,
    pub type_filter: String,
    pub hide_at_lvl: JsNumber,
    pub active_tags: Vec<String>,
    pub vault_only: bool,
    pub ducats_only: bool,
    pub min_vol: JsNumber,
    pub min_median: JsNumber,
    /// Any of these row types; empty restricts nothing.
    pub types_any: Vec<String>,
    pub spares_only: bool,
    /// Only rows the advisor has a verdict for.
    pub advice_only: bool,
}

/// What the table keeps of a row's `score_inventory` fact.
#[derive(Debug, Clone, Deserialize, TS)]
pub struct SellFilterFact {
    pub sellable: JsNumber,
    pub sell_score: JsNumber,
    pub timing: Timing,
}

/// One owned record that has a market entry.
#[derive(Debug, Clone, Deserialize, TS)]
pub struct SellFilterRow {
    pub count: JsNumber,
    #[serde(rename = "type")]
    pub item_type: Option<String>,
    /// `null` is no individual copy at all, which always shows.
    pub kept_lvl: Option<JsNumber>,
    pub leveled: JsNumber,
    pub subtype: Option<String>,
    pub avg: JsNumber,
    pub vol: JsNumber,
    pub median_90d: JsNumber,
    pub low5_avg: JsNumber,
    pub ducats: Option<JsNumber>,
    /// Indices into the request's `tags`.
    pub tags: Vec<usize>,
    pub vault: Option<String>,
    pub advice: Option<Advice>,
    /// Absent when the native scorer has not answered for the row.
    #[serde(default)]
    #[ts(optional)]
    pub fact: Option<SellFilterFact>,
    /// The protected allocation capping the fact's quantity; absent is uncapped.
    #[serde(default)]
    #[ts(optional)]
    pub allocation: Option<JsNumber>,
}

/// The rows' market tags each once: an inventory shares a few hundred tags
/// across its rows, so each is sent and folded once.
#[derive(Debug, Clone, Deserialize, TS)]
pub struct SellFilterRequest {
    pub tags: Vec<String>,
    pub rows: Vec<SellFilterRow>,
    pub filters: SellFilters,
}

/// A row that passed, by its index in the request.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
pub struct SellResult {
    pub index: usize,
    pub sellable: f64,
    pub potential_plat: f64,
    pub raw_value: f64,
    pub timing: Timing,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
pub struct AvailableTag {
    pub tag: String,
    pub count: usize,
    pub group: TagGroup,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum EmptyKind {
    NoMarket,
    Price,
    Owned,
    Type,
    Kept,
    Tag,
    Vault,
    Ducats,
    Vol,
    Median,
    Spares,
    Advice,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
pub struct EmptyReason {
    pub kind: EmptyKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub excluded: Option<usize>,
    pub candidates: usize,
}

/// The filters, with the request's tags folded to canonical ids once.
struct Cascade<'a> {
    f: &'a SellFilters,
    names: Vec<String>,
    /// Canonical id of each request tag.
    canonical: Vec<usize>,
    active: Vec<bool>,
}

impl<'a> Cascade<'a> {
    fn new(request: &'a SellFilterRequest) -> Self {
        let f = &request.filters;
        let mut names: Vec<String> = Vec::new();
        let mut ids: HashMap<String, usize> = HashMap::new();
        let canonical = request
            .tags
            .iter()
            .map(|tag| {
                *ids.entry(tags::canonical_tag(tag))
                    .or_insert_with_key(|tag| {
                        names.push(tag.clone());
                        names.len() - 1
                    })
            })
            .collect();
        let active = names.iter().map(|n| f.active_tags.contains(n)).collect();
        Cascade {
            f,
            names,
            canonical,
            active,
        }
    }

    /// The row's canonical tag ids, each once, in first-seen order.
    fn row_tags(&self, r: &SellFilterRow) -> Vec<usize> {
        let mut out: Vec<usize> = Vec::with_capacity(r.tags.len());
        for id in r
            .tags
            .iter()
            .filter_map(|&i| self.canonical.get(i).copied())
        {
            if !out.contains(&id) {
                out.push(id);
            }
        }
        out
    }
}

type Clause = fn(&SellFilterRow, &Cascade) -> bool;

fn passes_price(r: &SellFilterRow, c: &Cascade) -> bool {
    r.avg.0 >= c.f.min_price.0
}
fn passes_owned(r: &SellFilterRow, c: &Cascade) -> bool {
    r.count.0 >= c.f.min_owned.0
}
fn passes_type(r: &SellFilterRow, c: &Cascade) -> bool {
    let item_type = r.item_type.as_ref();
    if !c.f.types_any.is_empty() && !item_type.is_some_and(|t| c.f.types_any.contains(t)) {
        return false;
    }
    c.f.type_filter == "all" || item_type == Some(&c.f.type_filter)
}
fn passes_advice(r: &SellFilterRow, c: &Cascade) -> bool {
    !c.f.advice_only || r.advice.is_some()
}
fn passes_spares(r: &SellFilterRow, c: &Cascade) -> bool {
    let kept = r.kept_lvl.map(|k| k.0).filter(|k| k.is_finite());
    !c.f.spares_only || sell_priority::spare_qty(r.count.0, kept, r.leveled.0) >= 1.0
}
fn passes_kept(r: &SellFilterRow, c: &Cascade) -> bool {
    r.kept_lvl.is_none_or(|k| k.0 < c.f.hide_at_lvl.0)
}
fn passes_tag(r: &SellFilterRow, c: &Cascade) -> bool {
    c.f.active_tags.is_empty()
        || r.tags
            .iter()
            .filter_map(|&i| c.canonical.get(i))
            .any(|&id| c.active.get(id).copied().unwrap_or(false))
}
fn passes_vault(r: &SellFilterRow, c: &Cascade) -> bool {
    !c.f.vault_only || matches!(r.vault.as_deref(), Some("vaulted" | "vaulting-soon"))
}
fn passes_ducats(r: &SellFilterRow, c: &Cascade) -> bool {
    // The 'prime' tag alone also matches syndicate augments for prime weapons
    // (gilded_truth is tagged burston_prime), which have no ducat value - a
    // "best ducat value" list showing "Ducats: -" rows is nonsense.
    !c.f.ducats_only || (r.subtype.as_deref().is_none_or(str::is_empty) && r.ducats.is_some())
}
fn passes_vol(r: &SellFilterRow, c: &Cascade) -> bool {
    c.f.min_vol.0 <= 0.0 || js::or_zero(r.vol.0) >= c.f.min_vol.0
}
fn passes_median(r: &SellFilterRow, c: &Cascade) -> bool {
    c.f.min_median.0 <= 0.0 || js::or_zero(r.median_90d.0) >= c.f.min_median.0
}

/// The cascade in diagnosis order; ties in the empty reason go to the earlier.
const CLAUSES: [(EmptyKind, Clause); 11] = [
    (EmptyKind::Price, passes_price),
    (EmptyKind::Owned, passes_owned),
    (EmptyKind::Type, passes_type),
    (EmptyKind::Kept, passes_kept),
    (EmptyKind::Tag, passes_tag),
    (EmptyKind::Vault, passes_vault),
    (EmptyKind::Ducats, passes_ducats),
    (EmptyKind::Vol, passes_vol),
    (EmptyKind::Median, passes_median),
    (EmptyKind::Spares, passes_spares),
    (EmptyKind::Advice, passes_advice),
];

/// Every clause but the tag chips', which the chip counts leave out: a count
/// that assumed its own chip would overstate what clicking it yields.
fn passes_except_tag(r: &SellFilterRow, c: &Cascade) -> bool {
    CLAUSES
        .iter()
        .all(|(kind, passes)| *kind == EmptyKind::Tag || passes(r, c))
}

/// The Sell table's rows, best native score first (ties keep request order).
/// A row the native scorer has not answered for is not shown: the table never
/// scores on its own.
pub fn sell_results(request: &SellFilterRequest) -> Vec<SellResult> {
    let c = Cascade::new(request);
    let mut out: Vec<(f64, SellResult)> = Vec::new();
    for (index, r) in request.rows.iter().enumerate() {
        if !passes_except_tag(r, &c) || !passes_tag(r, &c) {
            continue;
        }
        let Some(fact) = &r.fact else { continue };
        let sellable = js::min(fact.sellable.0, r.allocation.map_or(f64::INFINITY, |a| a.0));
        // Raw stack value at the ~5 cheapest live asks, falling back to the
        // 48h closed average on snapshots that predate low5_avg.
        let unit = if js::or_zero(r.low5_avg.0) > 0.0 {
            r.low5_avg.0
        } else {
            r.avg.0
        };
        // Near its 90-day low usually means "wait": over the year to 2026-10, a
        // dip won back ~20% within four weeks. Where the advisor says sell now
        // (a new prime, or a Resurgence reprint and its relic companions) the
        // same dip kept falling instead, so the generic hold would contradict it.
        let timing = if fact.timing == Timing::Hold && r.advice == Some(Advice::SellNow) {
            Timing::Neutral
        } else {
            fact.timing.clone()
        };
        out.push((
            fact.sell_score.0,
            SellResult {
                index,
                sellable,
                potential_plat: sellable * r.avg.0,
                raw_value: sellable * unit,
                timing,
            },
        ));
    }
    // A stable sort on a total order: -0 is 0 as JS compares it, and a NaN
    // score (score_inventory never gives one) sinks rather than leaving the
    // order undefined, as JS would.
    let key = |score: f64| {
        if score.is_nan() {
            f64::NEG_INFINITY
        } else {
            score + 0.0
        }
    };
    out.sort_by(|(a, _), (b, _)| key(*b).total_cmp(&key(*a)));
    out.into_iter().map(|(_, row)| row).collect()
}

/// Each canonical tag on a row that passes every other clause, with its row
/// count, in first-seen order. The page orders them for its locale.
pub fn available_tags(request: &SellFilterRequest) -> Vec<AvailableTag> {
    let c = Cascade::new(request);
    let mut counts = vec![0_usize; c.names.len()];
    let mut order: Vec<usize> = Vec::new();
    for r in request.rows.iter().filter(|r| passes_except_tag(r, &c)) {
        for id in c.row_tags(r) {
            if let Some(count) = counts.get_mut(id) {
                if *count == 0 {
                    order.push(id);
                }
                *count += 1;
            }
        }
    }
    order
        .into_iter()
        .filter_map(|id| {
            let tag = c.names.get(id)?;
            Some(AvailableTag {
                tag: tag.clone(),
                count: *counts.get(id)?,
                group: tags::tag_group(tag),
            })
        })
        .collect()
}

/// Which single clause excludes the most rows, for the empty state's "relax
/// this filter". Every clause is walked on every row, so a row counts against
/// each clause it fails: a diagnostic, not a filter.
pub fn empty_reason(request: &SellFilterRequest) -> EmptyReason {
    let c = Cascade::new(request);
    let candidates = request.rows.len();
    if candidates == 0 {
        return EmptyReason {
            kind: EmptyKind::NoMarket,
            excluded: None,
            candidates,
        };
    }
    let mut top = (EmptyKind::Price, 0);
    for (index, (kind, passes)) in CLAUSES.iter().enumerate() {
        let excluded = request.rows.iter().filter(|r| !passes(r, &c)).count();
        if index == 0 || excluded > top.1 {
            top = (*kind, excluded);
        }
    }
    EmptyReason {
        kind: top.0,
        excluded: Some(top.1),
        candidates,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn filters(overrides: Value) -> SellFilters {
        let mut base = json!({
            "min_price": 0, "min_owned": 1, "type_filter": "all", "hide_at_lvl": 11,
            "active_tags": [], "vault_only": false, "ducats_only": false, "min_vol": 0,
            "min_median": 0, "types_any": [], "spares_only": false, "advice_only": false,
        });
        for (k, v) in overrides.as_object().unwrap() {
            base[k] = v.clone();
        }
        serde_json::from_value(base).unwrap()
    }

    fn row(overrides: Value) -> Value {
        let mut base = json!({
            "count": 1, "type": "Mods", "kept_lvl": null, "leveled": 0, "subtype": null,
            "avg": 50, "vol": 10, "median_90d": "NaN", "low5_avg": "NaN", "ducats": null,
            "tags": [], "vault": null, "advice": null,
            "fact": { "sellable": 1, "sell_score": 10, "timing": "neutral" },
        });
        for (k, v) in overrides.as_object().unwrap() {
            base[k] = v.clone();
        }
        base
    }

    // Rows give their tags by name; the request interns them.
    fn request(rows: &[Value], f: &SellFilters) -> SellFilterRequest {
        let mut names: Vec<String> = Vec::new();
        let rows = rows
            .iter()
            .map(|r| {
                let mut r = r.clone();
                let ids: Vec<usize> = r["tags"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|t| {
                        let t = t.as_str().unwrap().to_owned();
                        names.iter().position(|n| *n == t).unwrap_or_else(|| {
                            names.push(t);
                            names.len() - 1
                        })
                    })
                    .collect();
                r["tags"] = json!(ids);
                serde_json::from_value(r).unwrap()
            })
            .collect();
        SellFilterRequest {
            tags: names,
            rows,
            filters: f.clone(),
        }
    }

    fn results(rows: &[Value], f: &SellFilters) -> Vec<SellResult> {
        sell_results(&request(rows, f))
    }

    fn indices(rows: &[Value], f: &SellFilters) -> Vec<usize> {
        results(rows, f).iter().map(|r| r.index).collect()
    }

    #[test]
    fn each_clause_excludes_what_it_names() {
        let none: Vec<usize> = vec![];
        let one = |r: Value, f: Value| indices(&[row(r)], &filters(f));
        assert_eq!(one(json!({"avg": 3}), json!({"min_price": 5})), none);
        assert_eq!(one(json!({"count": 1}), json!({"min_owned": 2})), none);
        assert_eq!(one(json!({"kept_lvl": 5}), json!({"hide_at_lvl": 5})), none);
        assert_eq!(
            one(json!({"kept_lvl": 5}), json!({"hide_at_lvl": "Infinity"})),
            [0]
        );
        // A record restored without its kept level fails, as `undefined < n` does.
        assert_eq!(one(json!({"kept_lvl": "NaN"}), json!({})), none);
        assert_eq!(
            one(json!({"tags": ["mod"]}), json!({"active_tags": ["prime"]})),
            none
        );
        assert_eq!(
            one(
                json!({"tags": ["prime"]}),
                json!({"active_tags": ["prime"]})
            ),
            [0]
        );
        assert_eq!(one(json!({"avg": "NaN"}), json!({})), none);
        assert_eq!(
            one(json!({"type": "Arcanes"}), json!({"type_filter": "Mods"})),
            none
        );
        assert_eq!(one(json!({"vol": 9}), json!({"min_vol": 10})), none);
        assert_eq!(one(json!({"vol": "NaN"}), json!({"min_vol": 0})), [0]);
        assert_eq!(
            one(json!({"median_90d": 4}), json!({"min_median": 5})),
            none
        );
    }

    #[test]
    fn vault_ducat_and_advice_presets_keep_their_rows() {
        let rows = [
            row(json!({"vault": "vaulted"})),
            row(json!({"vault": "available"})),
            row(json!({"vault": "vaulting-soon"})),
        ];
        assert_eq!(
            indices(&rows, &filters(json!({"vault_only": true}))),
            [0, 2]
        );
        let rows = [
            row(json!({"ducats": 45})),
            row(json!({"ducats": null})),
            row(json!({"ducats": 45, "subtype": "radiant"})),
            row(json!({"ducats": 45, "subtype": ""})),
        ];
        assert_eq!(
            indices(&rows, &filters(json!({"ducats_only": true}))),
            [0, 3]
        );
        let rows = [row(json!({"advice": "hold"})), row(json!({}))];
        assert_eq!(indices(&rows, &filters(json!({"advice_only": true}))), [0]);
    }

    #[test]
    fn only_rows_the_native_scorer_answered_for_show_best_first() {
        let rows = [
            row(json!({"fact": {"sellable": 1, "sell_score": 75, "timing": "neutral"}})),
            row(json!({"fact": null})),
            row(json!({"fact": {"sellable": 1, "sell_score": 125, "timing": "neutral"}})),
            row(json!({"fact": {"sellable": 1, "sell_score": 75, "timing": "neutral"}})),
        ];
        assert_eq!(indices(&rows, &filters(json!({}))), [2, 0, 3]);
    }

    #[test]
    fn caps_the_native_quantity_at_the_allocation_and_revalues_it() {
        let fact = json!({"sellable": 4, "sell_score": 10, "timing": "neutral"});
        let capped = results(
            &[row(
                json!({"count": 5, "low5_avg": 60, "fact": fact, "allocation": 2}),
            )],
            &filters(json!({})),
        );
        assert_eq!(
            (
                capped[0].sellable,
                capped[0].potential_plat,
                capped[0].raw_value
            ),
            (2.0, 100.0, 120.0)
        );
        let unknown = results(
            &[row(json!({"fact": fact, "allocation": 0}))],
            &filters(json!({})),
        );
        assert_eq!(unknown[0].sellable, 0.0);
        let uncapped = results(&[row(json!({"fact": fact}))], &filters(json!({})));
        assert_eq!((uncapped[0].sellable, uncapped[0].raw_value), (4.0, 200.0));
    }

    #[test]
    fn spares_keep_mods_and_arcanes_with_a_spare_copy() {
        let rows = [
            row(json!({"count": 4, "kept_lvl": 5})),
            row(json!({"count": 3})),
            row(json!({"count": 1})),
            row(json!({"count": 2, "type": "Arcanes"})),
            row(json!({"count": 3, "type": "Warframes"})),
            row(json!({"count": 3, "kept_lvl": 10, "leveled": 3})),
        ];
        let spares = filters(json!({"types_any": ["Mods", "Arcanes"], "spares_only": true}));
        assert_eq!(indices(&rows, &spares), [0, 1, 3]);
        let lonely = [row(json!({"count": 1})), row(json!({"count": 1}))];
        assert_eq!(
            empty_reason(&request(&lonely, &spares)).kind,
            EmptyKind::Spares
        );
    }

    #[test]
    fn a_sell_now_verdict_replaces_the_near_low_hold() {
        let timing = |advice: Value| {
            let fact = json!({"sellable": 1, "sell_score": 10, "timing": "hold"});
            results(
                &[row(json!({"fact": fact, "advice": advice}))],
                &filters(json!({})),
            )[0]
            .timing
            .clone()
        };
        assert_eq!(timing(Value::Null), Timing::Hold);
        assert_eq!(timing(json!("hold")), Timing::Hold);
        assert_eq!(timing(json!("sell_now")), Timing::Neutral);
    }

    #[test]
    fn tag_counts_fold_breeds_and_skip_only_their_own_clause() {
        let rows = [
            row(json!({"tags": ["mod", "huras_kubrow"]})),
            row(json!({"tags": ["mod", "kubrow", "sunika_kubrow"]})),
            row(json!({"tags": ["kubrow_claws"]})),
            row(json!({"tags": ["prime"], "avg": 1})),
        ];
        let f = filters(json!({"active_tags": ["kubrow"], "min_price": 5}));
        let counts: Vec<_> = available_tags(&request(&rows, &f))
            .into_iter()
            .map(|t| (t.tag, t.count, t.group))
            .collect();
        assert_eq!(
            counts,
            [
                ("mod".into(), 2, TagGroup::Type),
                ("kubrow".into(), 2, TagGroup::Companion),
                ("kubrow_claws".into(), 1, TagGroup::Slot),
            ]
        );
        assert_eq!(indices(&rows, &f), [0, 1]);
    }

    #[test]
    fn the_empty_reason_names_the_clause_excluding_the_most() {
        assert_eq!(
            empty_reason(&request(&[], &filters(json!({})))),
            EmptyReason {
                kind: EmptyKind::NoMarket,
                excluded: None,
                candidates: 0
            }
        );
        let rows = [row(json!({"avg": 5})), row(json!({"avg": 5}))];
        assert_eq!(
            empty_reason(&request(&rows, &filters(json!({"min_price": 999})))),
            EmptyReason {
                kind: EmptyKind::Price,
                excluded: Some(2),
                candidates: 2
            }
        );
        assert_eq!(
            empty_reason(&request(&rows, &filters(json!({"advice_only": true})))).kind,
            EmptyKind::Advice
        );
        // Nothing excluded at all still names the first clause.
        assert_eq!(
            empty_reason(&request(&rows, &filters(json!({})))),
            EmptyReason {
                kind: EmptyKind::Price,
                excluded: Some(0),
                candidates: 2
            }
        );
        // A tie goes to the earlier clause.
        let tie = [row(json!({"avg": 1, "count": 0}))];
        assert_eq!(
            empty_reason(&request(&tie, &filters(json!({"min_price": 5})))).kind,
            EmptyKind::Price
        );
    }
}
