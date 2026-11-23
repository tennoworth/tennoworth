//! Ledger arithmetic over EE.log-confirmed trades. The trades arrive as the
//! desktop's own trade store wrote them, so every number is a plain JSON
//! number. Weeks stay with the page: they are local calendar weeks, and only
//! the page knows the user's time zone.

use crate::js;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum TradeKind {
    Sale,
    Purchase,
    Trade,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum ItemDirection {
    Given,
    Received,
}

#[derive(Debug, Clone, Deserialize, TS)]
pub struct LedgerItem {
    pub name: String,
    pub qty: f64,
    pub direction: ItemDirection,
}

#[derive(Debug, Clone, Deserialize, TS)]
pub struct LedgerTrade {
    pub kind: TradeKind,
    pub plat: f64,
    pub items: Vec<LedgerItem>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct LedgerTotals {
    pub sales: usize,
    pub purchases: usize,
    /// Plat received on sales.
    pub plat_in: f64,
    /// Plat spent on purchases.
    pub plat_out: f64,
    pub net: f64,
}

pub fn totals(trades: &[LedgerTrade]) -> LedgerTotals {
    let mut t = LedgerTotals::default();
    for trade in trades {
        match trade.kind {
            TradeKind::Sale => {
                t.sales += 1;
                t.plat_in += trade.plat;
            }
            TradeKind::Purchase => {
                t.purchases += 1;
                t.plat_out += trade.plat;
            }
            TradeKind::Trade => {}
        }
    }
    t.net = t.plat_in - t.plat_out;
    t
}

#[derive(Debug, Clone, Deserialize, TS)]
pub struct LedgerSinceRequest {
    /// Each trade's time, Unix seconds.
    pub at: Vec<f64>,
    pub days: f64,
    pub now_secs: f64,
}

/// The trades at or after `now_secs - days * 86400`, by index.
pub fn since(request: &LedgerSinceRequest) -> Vec<usize> {
    let cutoff = request.now_secs - request.days * 86_400.0;
    request
        .at
        .iter()
        .enumerate()
        .filter(|(_, at)| **at >= cutoff)
        .map(|(i, _)| i)
        .collect()
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
pub struct SoldItemTotal {
    pub name: String,
    pub qty: f64,
    /// Plat attributed to this item: whole-trade plat when it was the only
    /// item given, else split evenly by quantity across the given items - an
    /// honest approximation, labelled as such in the UI.
    pub plat: f64,
    pub trades: usize,
}

/// Realised plat per item across sales, rounded, in first-sold order. The
/// page ranks them, because ties fall to a locale's name order.
pub fn sold_by_item(trades: &[LedgerTrade]) -> Vec<SoldItemTotal> {
    let mut out: Vec<SoldItemTotal> = Vec::new();
    let mut index: HashMap<&str, usize> = HashMap::new();
    for trade in trades.iter().filter(|t| t.kind == TradeKind::Sale) {
        let given: Vec<&LedgerItem> = trade
            .items
            .iter()
            .filter(|i| i.direction == ItemDirection::Given)
            .collect();
        let units = given.iter().fold(0.0, |n, i| n + js::max(1.0, i.qty));
        for item in given {
            let share = if units > 0.0 {
                trade.plat * js::max(1.0, item.qty) / units
            } else {
                0.0
            };
            let at = *index.entry(&item.name).or_insert_with(|| {
                out.push(SoldItemTotal {
                    name: item.name.clone(),
                    qty: 0.0,
                    plat: 0.0,
                    trades: 0,
                });
                out.len() - 1
            });
            if let Some(row) = out.get_mut(at) {
                row.qty += item.qty;
                row.plat += share;
                row.trades += 1;
            }
        }
    }
    for row in &mut out {
        row.plat = js::round(row.plat);
    }
    out
}

#[derive(Debug, Clone, Deserialize, TS)]
pub struct TopSellersRequest {
    /// The ranked sellers' plat, best first.
    pub plats: Vec<f64>,
    pub count: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
pub struct OtherSellers {
    pub items: usize,
    pub plat: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
pub struct TopSellers {
    /// How many of the ranked sellers to show.
    pub top: usize,
    pub other: Option<OtherSellers>,
}

/// The first `count` sellers, and what the rest add up to as one "Other" row,
/// so a long tail neither crowds the ranking nor drops out of the total.
pub fn top_sellers(request: &TopSellersRequest) -> TopSellers {
    let top = js::slice_index(request.count, request.plats.len());
    let rest = request.plats.get(top..).unwrap_or_default();
    TopSellers {
        top,
        other: (!rest.is_empty()).then(|| OtherSellers {
            items: rest.len(),
            plat: rest.iter().fold(0.0, |n, p| n + p),
        }),
    }
}

#[derive(Debug, Clone, Deserialize, TS)]
pub struct WeekPlat {
    pub plat: f64,
    /// The current week, still running.
    pub partial: bool,
}

/// Median plat of the finished weeks; `None` when there are none.
pub fn median_week(weeks: &[WeekPlat]) -> Option<f64> {
    let mut done: Vec<f64> = weeks
        .iter()
        .filter(|w| !w.partial)
        .map(|w| w.plat)
        .collect();
    done.sort_by(f64::total_cmp);
    let mid = done.len() / 2;
    if done.len() % 2 == 1 {
        done.get(mid).copied()
    } else {
        Some((done.get(mid.checked_sub(1)?)? + done.get(mid)?) / 2.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn trades() -> Vec<LedgerTrade> {
        serde_json::from_value(json!([
            {"kind": "sale", "plat": 40, "items": [{"name": "Primed Flow", "qty": 1, "direction": "given"}]},
            {"kind": "sale", "plat": 30, "items": [{"name": "Lith C5 Relic", "qty": 3, "direction": "given"}]},
            {"kind": "purchase", "plat": 25, "items": [{"name": "Ash Prime Blueprint", "qty": 1, "direction": "received"}]},
            {"kind": "trade", "plat": 0, "items": [{"name": "A", "qty": 1, "direction": "given"}, {"name": "B", "qty": 1, "direction": "received"}]},
            {"kind": "sale", "plat": 90, "items": [{"name": "Primed Flow", "qty": 1, "direction": "given"}, {"name": "Lith C5 Relic", "qty": 2, "direction": "given"}]},
        ]))
        .unwrap()
    }

    #[test]
    fn totals_sales_purchases_and_net() {
        assert_eq!(
            totals(&trades()),
            LedgerTotals {
                sales: 3,
                purchases: 1,
                plat_in: 160.0,
                plat_out: 25.0,
                net: 135.0
            }
        );
        assert_eq!(totals(&[]), LedgerTotals::default());
    }

    #[test]
    fn since_slices_by_age() {
        let request = LedgerSinceRequest {
            at: vec![1000.0, 2000.0, 3000.0, 4000.0, 5000.0],
            days: 1.0,
            now_secs: 4000.0 + 86_400.0,
        };
        assert_eq!(since(&request), [3, 4]);
    }

    #[test]
    fn sold_by_item_splits_multi_item_sales_by_quantity() {
        // The last sale: 90p over 1 Primed Flow + 2 relics = 30p a unit.
        let sold = sold_by_item(&trades());
        assert_eq!(
            sold,
            [
                SoldItemTotal {
                    name: "Primed Flow".into(),
                    qty: 2.0,
                    plat: 70.0,
                    trades: 2
                },
                SoldItemTotal {
                    name: "Lith C5 Relic".into(),
                    qty: 5.0,
                    plat: 90.0,
                    trades: 2
                },
            ]
        );
        // A zero-quantity line still counts as one unit, and halves round up.
        let odd: Vec<LedgerTrade> = serde_json::from_value(json!([
            {"kind": "sale", "plat": 5, "items": [{"name": "X", "qty": 0, "direction": "given"}, {"name": "Y", "qty": 1, "direction": "given"}]},
        ])).unwrap();
        let sold = sold_by_item(&odd);
        assert_eq!((sold[0].qty, sold[0].plat, sold[1].plat), (0.0, 3.0, 3.0));
    }

    #[test]
    fn the_tail_of_top_sellers_folds_into_one_other_row() {
        let plats = vec![10.0, 20.0, 30.0, 40.0];
        assert_eq!(
            top_sellers(&TopSellersRequest {
                plats: plats.clone(),
                count: 2.0
            }),
            TopSellers {
                top: 2,
                other: Some(OtherSellers {
                    items: 2,
                    plat: 70.0
                })
            }
        );
        assert_eq!(
            top_sellers(&TopSellersRequest { plats, count: 8.0 }),
            TopSellers {
                top: 4,
                other: None
            }
        );
    }

    #[test]
    fn the_median_week_counts_finished_weeks_only() {
        let w = |plat: f64, partial: bool| WeekPlat { plat, partial };
        assert_eq!(
            median_week(&[
                w(10.0, false),
                w(30.0, false),
                w(20.0, false),
                w(500.0, true)
            ]),
            Some(20.0)
        );
        assert_eq!(median_week(&[w(10.0, false), w(30.0, false)]), Some(20.0));
        assert_eq!(median_week(&[w(9.0, true)]), None);
    }
}
