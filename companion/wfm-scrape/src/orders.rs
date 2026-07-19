//! Order-book parsing — the v2 `/v2/orders/item/{slug}` list.
//!
//! Python filters liveness FIRST (`[o for o in orders if live(o, kind)]`) and
//! only ever reads `platinum` on the survivors — it never touches the field on
//! an offline/invisible/other-side order. We mirror that order exactly: the
//! liveness predicate runs on the raw order objects, and only a live order's
//! `platinum` flows through [`crate::coerce`]. Junk (`{}` / `true` / a
//! numeric-string) in a dead order is therefore never coerced, so it can
//! neither consume the coercion budget nor abort the run — precisely because
//! Python never reads it either.

use serde_json::Value;

use market_math::LiveOrder;

use crate::coerce::{coerce_field, Coercions};

/// One LIVE order of a tradable side, retaining its side (`type`) so the caller
/// can split buys from sells. Only live orders are ever built — liveness was
/// already applied in [`parse_orders`].
#[derive(Debug, Clone)]
pub struct ParsedOrder {
    pub otype: String,
    pub order: LiveOrder,
}

/// Python's `live(o, kind)` minus the side test: an in-game/online seller whose
/// listing is visible. The side (`type`) is matched separately in
/// [`parse_orders`], so one pass keeps both buy and sell survivors.
fn is_live(o: &Value) -> bool {
    // Python: (o.get("user") or {}).get("status") in ("ingame", "online")
    let status_ok = matches!(
        o.get("user").and_then(|u| u.get("status")).and_then(|s| s.as_str()),
        Some("ingame") | Some("online")
    );
    // Python: o.get("visible", True) — absent → visible; a present value is
    // truth-tested, so an explicit false/null is NOT live.
    let visible = match o.get("visible") {
        None => true,
        Some(v) => v.as_bool().unwrap_or(false),
    };
    status_ok && visible
}

/// Parse the (already envelope-unwrapped) order list into its LIVE buy/sell
/// orders. Non-array input and non-object entries are skipped — the same shape
/// tolerance Python's `for o in orders` has once `data` is a list.
///
/// FIELD-READ PARITY: the liveness filter (side ∈ {buy, sell}, status, visible)
/// runs BEFORE any coercion, so `platinum` is coerced only on the exact orders
/// Python reads it on — never on a dead one.
pub fn parse_orders(orders: &Value, url_name: &str, co: &mut Coercions) -> Result<Vec<ParsedOrder>, String> {
    let arr = match orders.as_array() {
        Some(a) => a,
        None => return Ok(vec![]),
    };
    let mut out = Vec::new();
    for (i, o) in arr.iter().enumerate() {
        if !o.is_object() {
            continue;
        }
        // Python reads platinum only inside live_buys / live_sells, i.e. only on
        // orders whose type is the side being scanned. An other-typed order is
        // in neither list, so its fields are never touched — drop it before the
        // coercion, matching that.
        let otype = o.get("type").and_then(|t| t.as_str()).unwrap_or("");
        if otype != "buy" && otype != "sell" {
            continue;
        }
        if !is_live(o) {
            continue;
        }
        let path = format!("{url_name}.orders[{i}]");
        let platinum = coerce_field(o, "platinum", &format!("{path}.platinum"), co)?;
        out.push(ParsedOrder {
            otype: otype.to_string(),
            order: LiveOrder {
                platinum,
                rank: parse_rank(o.get("rank")),
                subtype: o.get("subtype").and_then(|s| s.as_str()).map(|s| s.to_string()),
            },
        });
    }
    Ok(out)
}

/// v2 order `rank`: absent OR null → `None` (untiered); a number → `Some(n)`.
/// Both absent and explicit-null collapse to `None`, matching Python's
/// `o.get("rank") is not None` liveness of the tier check.
fn parse_rank(v: Option<&Value>) -> Option<i64> {
    match v {
        None | Some(Value::Null) => None,
        Some(x) => x.as_i64().or_else(|| x.as_f64().map(|f| f as i64)),
    }
}

/// Split out the live orders of one side (`"buy"` / `"sell"`). Liveness was
/// already applied in [`parse_orders`]; here we only partition by side — the
/// last piece of Python's `[o for o in orders if live(o, kind)]`. Returns the
/// bare [`LiveOrder`]s the market-math tier filters operate on.
pub fn live_orders(parsed: &[ParsedOrder], kind: &str) -> Vec<LiveOrder> {
    parsed
        .iter()
        .filter(|o| o.otype == kind)
        .map(|o| o.order.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn parse(v: &Value) -> Vec<ParsedOrder> {
        let mut co = Coercions::new();
        parse_orders(v, "slug", &mut co).unwrap()
    }

    #[test]
    fn keeps_only_live_visible_ingame_or_online_orders_of_the_right_side() {
        let v = json!([
            {"type": "buy", "platinum": 30, "visible": true, "user": {"status": "ingame"}},
            {"type": "buy", "platinum": 25, "visible": true, "user": {"status": "online"}},
            {"type": "buy", "platinum": 99, "visible": true, "user": {"status": "offline"}},
            {"type": "buy", "platinum": 50, "visible": false, "user": {"status": "ingame"}},
            {"type": "sell", "platinum": 40, "visible": true, "user": {"status": "ingame"}}
        ]);
        let parsed = parse(&v);
        let buys = live_orders(&parsed, "buy");
        assert_eq!(buys.iter().map(|o| o.platinum).collect::<Vec<_>>(), vec![30.0, 25.0]);
        let sells = live_orders(&parsed, "sell");
        assert_eq!(sells.iter().map(|o| o.platinum).collect::<Vec<_>>(), vec![40.0]);
    }

    #[test]
    fn visible_defaults_to_true_when_absent() {
        let v = json!([{"type": "sell", "platinum": 12, "user": {"status": "online"}}]);
        assert_eq!(live_orders(&parse(&v), "sell").len(), 1);
    }

    #[test]
    fn explicit_false_or_null_visible_is_not_live() {
        let v = json!([
            {"type": "sell", "platinum": 12, "visible": false, "user": {"status": "online"}},
            {"type": "sell", "platinum": 13, "visible": null, "user": {"status": "online"}}
        ]);
        assert!(live_orders(&parse(&v), "sell").is_empty());
    }

    #[test]
    fn missing_user_is_not_live() {
        let v = json!([{"type": "sell", "platinum": 12, "visible": true}]);
        assert!(live_orders(&parse(&v), "sell").is_empty());
    }

    #[test]
    fn rank_absent_or_null_is_untiered_number_is_a_tier() {
        let v = json!([
            {"type": "sell", "platinum": 10, "user": {"status": "ingame"}},
            {"type": "sell", "platinum": 20, "rank": null, "user": {"status": "ingame"}},
            {"type": "sell", "platinum": 30, "rank": 3, "user": {"status": "ingame"}}
        ]);
        let sells = live_orders(&parse(&v), "sell");
        assert_eq!(sells[0].rank, None);
        assert_eq!(sells[1].rank, None);
        assert_eq!(sells[2].rank, Some(3));
    }

    #[test]
    fn subtype_is_carried_through() {
        let v = json!([{"type": "sell", "platinum": 10, "subtype": "intact", "user": {"status": "ingame"}}]);
        assert_eq!(live_orders(&parse(&v), "sell")[0].subtype.as_deref(), Some("intact"));
    }

    #[test]
    fn non_array_and_non_object_entries_are_skipped() {
        assert!(parse(&json!({})).is_empty());
        let v = json!([1, "x", {"type": "sell", "platinum": 5, "user": {"status": "online"}}]);
        assert_eq!(parse(&v).len(), 1);
    }

    #[test]
    fn junk_platinum_in_dead_or_wrong_side_orders_is_never_coerced() {
        // The blocking bug this guards: Python filters liveness FIRST, so it
        // never reads `platinum` on an offline / invisible / other-side order.
        // A junk platinum there (object → hard error, numeric-string → counted)
        // must NOT abort the run or move the coercion budget — only the one live
        // survivor's platinum is read.
        let mut co = Coercions::new();
        let v = json!([
            {"type": "sell", "platinum": {"junk": 1}, "visible": true,  "user": {"status": "offline"}},
            {"type": "buy",  "platinum": [1, 2],      "visible": false, "user": {"status": "ingame"}},
            {"type": "trade","platinum": {"x": 1},    "visible": true,  "user": {"status": "ingame"}},
            {"type": "sell", "platinum": "77",        "visible": false, "user": {"status": "ingame"}},
            {"type": "sell", "platinum": 25,          "visible": true,  "user": {"status": "ingame"}}
        ]);
        let parsed = parse_orders(&v, "slug", &mut co).unwrap();
        assert_eq!(
            live_orders(&parsed, "sell").iter().map(|o| o.platinum).collect::<Vec<_>>(),
            vec![25.0]
        );
        // The "77" numeric-string sat in an invisible (dead) order → uncounted.
        assert_eq!(co.count, 0);
    }
}
