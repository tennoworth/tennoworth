use std::cmp::Ordering;

use market_math::sell_priority;

use crate::persistence::Db;

use super::{MarketData, ScanNotification, SellableRow};

/// Rank every sellable item in the latest snapshot, highest prioritization score first.
/// Items resolving to the same slug (rare) are aggregated. Rows with no market
/// entry, nothing left after the reserve, or a zero score are dropped. Sorted
/// by score desc, then slug asc for a deterministic order (the SPA uses
/// insertion order on ties; ties are irrelevant for a top-N tray).
pub fn rank_sellables(db: &Db, market: &MarketData) -> Vec<SellableRow> {
    let inventory = match market.sellable_inventory(db) {
        Ok(rows) => rows,
        Err(error) => {
            eprintln!("tennoworth: sellable quantities unavailable: {error}");
            return vec![];
        }
    };
    let mut rows: Vec<SellableRow> = Vec::new();
    for (slug, (name, safe)) in inventory {
        let Some(entry) = market.items.get(&slug) else {
            continue;
        };
        let sellable = i64::from(safe);
        if sellable <= 0 {
            continue;
        }
        let priced = entry.priced();
        let score =
            sell_priority::score_row_weighted(sellable as f64, &priced, market.usage_share(&slug));
        if score.sell_score <= 0.0 {
            continue;
        }
        rows.push(SellableRow {
            name,
            slug,
            sellable_qty: sellable,
            price: sell_priority::clearing_price(&priced),
            score: score.sell_score,
        });
    }

    rows.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(Ordering::Equal)
            .then_with(|| a.slug.cmp(&b.slug))
    });
    rows
}

/// The notification payload for a completed scan, or `None` when nothing is
/// sellable (→ the caller fires no notification). Total is the realizable plat
/// across the WHOLE sellable set, rounded - "N items worth ~Xp to sell".
pub fn build_notification(sellables: &[SellableRow]) -> Option<ScanNotification> {
    if sellables.is_empty() {
        return None;
    }
    let total: f64 = sellables
        .iter()
        .map(|r| r.price * r.sellable_qty as f64)
        .sum();
    Some(ScanNotification {
        count: sellables.len(),
        total_plat: total.round() as i64,
    })
}
