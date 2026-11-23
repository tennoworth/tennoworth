//! Which live buyers would take a stack, and what they would pay for it
//! against the listing's own ask. The desktop's Trade Session shows it per row;
//! the book comes from `wfm-trading`'s live check, which already drops the
//! account's own bids and keeps one order per buyer.

use crate::limits::MAX_PER_TRADE;
use crate::listing::sort_stably;
use crate::page::{self, deserialize_by_reading};
use serde::Serialize;
use serde_json::Value;
use ts_rs::TS;

/// A quote this old no longer describes the book.
pub const BUYER_QUOTE_TTL_MS: f64 = 60_000.0;

const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

fn is_safe_integer(x: Option<f64>) -> bool {
    x.is_some_and(|x| x.is_finite() && x.trunc() == x && x.abs() <= MAX_SAFE_INTEGER)
}

/// One buyer's order as the live check lists it.
#[derive(Debug, Clone, TS)]
#[ts(optional_fields = nullable)]
pub struct BuyerBid {
    pub id: Option<String>,
    pub user_id: Option<String>,
    pub status: Option<String>,
    pub quantity: Option<f64>,
    pub per_trade: Option<f64>,
    /// Total price for one complete lot.
    pub platinum: Option<f64>,
}

#[derive(Debug, Clone, TS)]
#[ts(optional_fields = nullable)]
pub struct BuyerBookQuote {
    pub orders: Option<Vec<BuyerBid>>,
    /// Whether the account was known, so its own bids are not in `orders`.
    pub own_orders_excluded: Option<bool>,
}

#[derive(Debug, Clone, TS)]
#[ts(optional_fields = nullable)]
pub struct BuyerQuote {
    /// Set when the lookup failed.
    pub error: Option<String>,
    pub buyer_book: Option<BuyerBookQuote>,
}

#[derive(Debug, Clone, TS)]
#[ts(optional_fields = nullable)]
pub struct BuyerCompareRequest {
    pub quote: Option<BuyerQuote>,
    pub quantity: Option<f64>,
    /// The listing's per-unit ask.
    pub ask: Option<f64>,
    /// Epoch milliseconds of the comparison and of the book's observation;
    /// the page reads both clocks, so the rule never does.
    pub now: Option<f64>,
    pub observed: Option<f64>,
}

impl BuyerCompareRequest {
    fn read(v: &Value) -> Self {
        let bid = |o: &Value| BuyerBid {
            id: page::text(o, "id"),
            user_id: page::text(o, "user_id"),
            status: page::text(o, "status"),
            quantity: page::number(o, "quantity"),
            per_trade: page::number(o, "per_trade"),
            platinum: page::number(o, "platinum"),
        };
        let book = |b: &Value| BuyerBookQuote {
            orders: page::list(b, "orders").map(|orders| orders.iter().map(bid).collect()),
            own_orders_excluded: page::flag(b, "own_orders_excluded"),
        };
        Self {
            quote: page::object(v, "quote").map(|q| BuyerQuote {
                error: page::text(q, "error"),
                buyer_book: page::object(q, "buyer_book").map(book),
            }),
            quantity: page::number(v, "quantity"),
            ask: page::number(v, "ask"),
            now: page::number(v, "now"),
            observed: page::number(v, "observed"),
        }
    }
}

deserialize_by_reading!(BuyerCompareRequest);

/// Why buyers cannot be compared; the view words each one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum BuyerCompareError {
    Invalid,
    Unavailable,
    Locked,
    Stale,
}

/// The whole lots one buyer takes.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, TS)]
pub struct BuyerFill {
    /// The buyer's position in the quote's `orders`.
    pub order: usize,
    pub quantity: f64,
    pub value: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
pub struct BuyerCoverage {
    pub rows: Vec<BuyerFill>,
    pub covered: f64,
    pub uncovered: f64,
    pub value: f64,
    /// The covered quantity at the listing's ask.
    pub ask_value: f64,
    pub gap: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(untagged)]
pub enum BuyerComparison {
    Failed { error: BuyerCompareError },
    Covered(BuyerCoverage),
}

fn eligible(order: &BuyerBid) -> Option<(f64, f64, f64)> {
    let (quantity, per_trade, platinum) = (order.quantity?, order.per_trade?, order.platinum?);
    let valid = order.id.as_deref().is_some_and(|id| !id.is_empty())
        && matches!(order.status.as_deref(), Some("online" | "ingame"))
        && is_safe_integer(order.quantity)
        && quantity > 0.0
        && is_safe_integer(order.per_trade)
        && per_trade >= 1.0
        && per_trade <= f64::from(MAX_PER_TRADE)
        && quantity % per_trade == 0.0
        && is_safe_integer(order.platinum)
        && platinum > 0.0;
    valid.then_some((quantity, per_trade, platinum))
}

/// Fill `quantity` from the best unit price down, one whole lot at a time,
/// and price the covered part at the ask too. Never multiplies the top bid
/// across a stack it cannot absorb. Equal unit prices go by order id in byte
/// order, the live check's own tie order.
pub fn compare_buyers(request: &BuyerCompareRequest) -> BuyerComparison {
    let failed = |error| BuyerComparison::Failed { error };
    let (Some(quantity), Some(ask)) = (request.quantity, request.ask) else {
        return failed(BuyerCompareError::Invalid);
    };
    if !is_safe_integer(request.quantity) || quantity <= 0.0 || !ask.is_finite() || ask <= 0.0 {
        return failed(BuyerCompareError::Invalid);
    }
    let quote = request
        .quote
        .as_ref()
        .filter(|q| q.error.as_deref().is_none_or(str::is_empty));
    let book = quote.and_then(|q| q.buyer_book.as_ref());
    let (Some(book), Some(orders), Some(observed)) = (
        book,
        book.and_then(|b| b.orders.as_ref()),
        request.observed.filter(|o| o.is_finite()),
    ) else {
        return failed(BuyerCompareError::Unavailable);
    };
    if book.own_orders_excluded != Some(true) {
        return failed(BuyerCompareError::Locked);
    }
    match request.now.filter(|now| now.is_finite()) {
        Some(now) if now >= observed && now - observed < BUYER_QUOTE_TTL_MS => {}
        _ => return failed(BuyerCompareError::Stale),
    }

    let mut seen: Vec<&str> = Vec::new();
    let mut buyers: Vec<(usize, &str, f64, f64, f64)> = orders
        .iter()
        .enumerate()
        .filter_map(|(index, order)| {
            let user = order.user_id.as_deref().filter(|u| !u.is_empty())?;
            if seen.contains(&user) {
                return None;
            }
            let (quantity, per_trade, platinum) = eligible(order)?;
            seen.push(user);
            Some((
                index,
                order.id.as_deref().unwrap_or_default(),
                quantity,
                per_trade,
                platinum,
            ))
        })
        .collect();
    // The page compared `b - a || id order`: a unit-price difference of zero
    // falls through to the id.
    sort_stably(&mut buyers, |a, b| {
        let better = a.4 / a.3 - b.4 / b.3;
        better > 0.0 || (better == 0.0 && a.1 < b.1)
    });

    let mut remaining = quantity;
    let mut rows = Vec::new();
    for (index, _, available, per_trade, platinum) in buyers {
        let units = (remaining.min(available) / per_trade).floor() * per_trade;
        if units == 0.0 {
            continue;
        }
        rows.push(BuyerFill {
            order: index,
            quantity: units,
            value: units / per_trade * platinum,
        });
        remaining -= units;
    }
    let covered = quantity - remaining;
    let value = rows.iter().fold(0.0, |sum, row| sum + row.value);
    BuyerComparison::Covered(BuyerCoverage {
        rows,
        covered,
        uncovered: remaining,
        value,
        ask_value: covered * ask,
        gap: covered * ask - value,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    const NOW: f64 = 1_788_868_800_000.0; // 2026-09-08T12:00:00Z

    fn buyer(id: &str, quantity: f64, platinum: f64, per_trade: f64) -> Value {
        json!({ "id": id, "user_id": id, "name": id, "user_slug": id, "status": "ingame", "platform": "pc", "crossplay": true,
            "quantity": quantity, "platinum": platinum, "per_trade": per_trade })
    }

    fn default_book() -> Vec<Value> {
        vec![buyer("a", 2.0, 12.0, 1.0), buyer("b", 3.0, 10.0, 1.0)]
    }

    fn quote(orders: Vec<Value>) -> Value {
        json!({ "slug": "part", "rank": 0, "subtype": null, "buys": [12, 10], "sells": [15], "low_sell": 15, "top_buy": 12,
            "buyer_book": { "orders": orders, "observed_at": "2026-09-08T12:00:00.000Z", "own_orders_excluded": true } })
    }

    fn compare(quote: Value, quantity: f64, ask: f64, now: f64) -> Value {
        let request = json!({ "quote": quote, "quantity": quantity, "ask": ask, "now": now, "observed": NOW });
        serde_json::to_value(compare_buyers(&BuyerCompareRequest::read(&request))).unwrap()
    }

    fn coverage(result: &Value) -> (f64, f64, f64) {
        (
            result["covered"].as_f64().unwrap(),
            result["uncovered"].as_f64().unwrap(),
            result["value"].as_f64().unwrap(),
        )
    }

    #[test]
    fn consumes_the_book_the_native_parser_verified() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/buyer-alternatives.json"
        ))
        .unwrap();
        let c = &fixture["comparison"];
        let result = compare(
            quote(fixture["orders"].as_array().unwrap().clone()),
            c["quantity"].as_f64().unwrap(),
            c["ask"].as_f64().unwrap(),
            NOW,
        );
        for (ours, theirs) in [
            ("covered", "covered"),
            ("uncovered", "uncovered"),
            ("value", "value"),
            ("gap", "gap"),
            ("ask_value", "askValue"),
        ] {
            assert_eq!(result[ours].as_f64(), c[theirs].as_f64(), "{ours}");
        }
    }

    #[test]
    fn prices_only_the_stack_the_buyers_cover() {
        let result = compare(quote(default_book()), 8.0, 15.0, NOW);
        assert_eq!(coverage(&result), (5.0, 3.0, 54.0));
        assert_eq!(
            (result["ask_value"].as_f64(), result["gap"].as_f64()),
            (Some(75.0), Some(21.0))
        );
        let four = compare(quote(default_book()), 4.0, 15.0, NOW);
        assert_eq!(coverage(&four), (4.0, 0.0, 44.0));
        assert_eq!(
            (four["ask_value"].as_f64(), four["gap"].as_f64()),
            (Some(60.0), Some(16.0))
        );
    }

    #[test]
    fn fills_whole_lots_only() {
        let result = compare(
            quote(vec![buyer("a", 6.0, 24.0, 2.0), buyer("b", 3.0, 10.0, 1.0)]),
            5.0,
            15.0,
            NOW,
        );
        assert_eq!(coverage(&result).0, 5.0);
        assert_eq!(
            result["rows"],
            json!([{ "order": 0, "quantity": 4.0, "value": 48.0 }, { "order": 1, "quantity": 1.0, "value": 10.0 }])
        );
        assert_eq!(
            coverage(&compare(
                quote(vec![buyer("a", 6.0, 24.0, 2.0)]),
                1.0,
                15.0,
                NOW
            )),
            (0.0, 1.0, 0.0)
        );
    }

    #[test]
    fn breaks_a_price_tie_by_order_id() {
        let result = compare(
            quote(vec![buyer("b", 1.0, 10.0, 1.0), buyer("a", 1.0, 10.0, 1.0)]),
            1.0,
            15.0,
            NOW,
        );
        assert_eq!(result["rows"][0]["order"], 1);
    }

    #[test]
    fn tells_an_empty_book_from_a_missing_or_failed_one() {
        assert_eq!(
            coverage(&compare(quote(vec![]), 3.0, 15.0, NOW)),
            (0.0, 3.0, 0.0)
        );
        let mut missing = quote(default_book());
        missing["buyer_book"] = Value::Null;
        assert_eq!(
            compare(missing, 3.0, 15.0, NOW),
            json!({ "error": "unavailable" })
        );
        let mut failed = quote(default_book());
        failed["error"] = json!("network failure");
        assert_eq!(
            compare(failed, 3.0, 15.0, NOW),
            json!({ "error": "unavailable" })
        );
    }

    #[test]
    fn refuses_stale_future_and_unidentified_quotes() {
        assert_eq!(
            compare(quote(default_book()), 3.0, 15.0, NOW + 60_000.0),
            json!({ "error": "stale" })
        );
        assert_eq!(
            compare(quote(default_book()), 3.0, 15.0, NOW - 1.0),
            json!({ "error": "stale" })
        );
        let mut logged_out = quote(default_book());
        logged_out["buyer_book"]["own_orders_excluded"] = json!(false);
        assert_eq!(
            compare(logged_out, 3.0, 15.0, NOW),
            json!({ "error": "locked" })
        );
    }

    #[test]
    fn rejects_an_invalid_stack_and_counts_each_buyer_once() {
        assert_eq!(
            compare(quote(default_book()), 1.5, 15.0, NOW),
            json!({ "error": "invalid" })
        );
        let infinite = json!({ "quote": quote(default_book()), "quantity": null, "ask": 15, "now": NOW, "observed": NOW });
        assert_eq!(
            serde_json::to_value(compare_buyers(&BuyerCompareRequest::read(&infinite))).unwrap(),
            json!({ "error": "invalid" })
        );
        let twice = compare(
            quote(vec![buyer("a", 2.0, 12.0, 1.0), buyer("a", 2.0, 12.0, 1.0)]),
            4.0,
            15.0,
            NOW,
        );
        assert_eq!(coverage(&twice).0, 2.0);
    }
}
