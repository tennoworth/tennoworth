//! Translation of this API's own-order responses into the portable order
//! contract.
//!
//! The endpoint has shipped two row shapes - a bucketed `{data:{sell,buy}}` and
//! a flat `{data:[...]}` - and states a row's side twice, as the bucket it is
//! filed under and as a field on the row. Consumers that read the raw JSON each
//! decided for themselves what a missing field meant and which side won, so one
//! row could be a sell on one surface and a buy on another. Decoding once, here,
//! is what stops that: a row is understood completely, understood not to be
//! supported, or not understood at all.

use anyhow::{bail, Context, Result};
use market_domain::orders::{ItemConstraints, NormalizedOrder, OrderRow, OrderSide, RefusedRow};

/// How many characters of the raw row an unsupported or ambiguous row keeps as
/// evidence. Enough for a banner the user can act on, bounded so a hostile or
/// merely enormous row cannot be carried around the app.
const MAX_EVIDENCE: usize = 512;

/// The decoded orders response: its rows, plus what the decoder could not
/// account for.
#[derive(Debug)]
pub struct DecodedOrders {
    pub orders: Vec<OrderRow>,
    /// Live orders this build cannot interpret. They are still the account's
    /// orders - a surface can list them from their evidence - but while any
    /// exist, the decoded rows are not a complete picture of the account.
    pub unsupported: usize,
    /// Rows whose own contents disagree with the response that carried them, or
    /// which are missing the fields an order cannot do without. These are the
    /// response contradicting itself rather than merely describing something
    /// new.
    pub ambiguous: usize,
}

impl DecodedOrders {
    /// An account with no orders on either side.
    pub fn empty() -> Self {
        Self {
            orders: Vec::new(),
            unsupported: 0,
            ambiguous: 0,
        }
    }
}

/// Decode a `/v2/orders/user/<username>` body.
///
/// Fails when the response cannot be read as a complete account: no `data`,
/// neither of the two known shapes, or any row this build cannot interpret.
/// The failure is the whole-body rejection - a partial reading must never be
/// reconciled as the whole account - and the two counters say which kind of
/// incompleteness caused it, so a future increment can relax exactly the one
/// that turns out to be safe.
pub fn decode_orders(
    body: &serde_json::Value,
    constraints: &dyn ItemConstraints,
) -> Result<DecodedOrders> {
    let data = body
        .get("data")
        .context("Orders response has no data; refusing to assume an empty account.")?;
    let mut orders = Vec::new();

    if let Some(rows) = data.as_array() {
        // The flat shape carries no bucket, so the row's own side is the only
        // statement of it and must be present.
        for row in rows {
            orders.push(decode_row(row, None, constraints));
        }
    } else if is_buckets(data) {
        for bucket in ["sell", "buy"] {
            let rows = data.get(bucket).and_then(|value| value.as_array());
            let side = side_from(bucket);
            for row in rows.into_iter().flatten() {
                orders.push(decode_row(row, side, constraints));
            }
        }
    } else {
        bail!("Orders response is neither a row list nor a pair of sell/buy buckets; reconcile current orders before changing listings.");
    }

    let ambiguous = orders
        .iter()
        .filter(|row| matches!(row, OrderRow::Ambiguous(_)))
        .count();
    let unsupported = orders
        .iter()
        .filter(|row| matches!(row, OrderRow::Unsupported(_)))
        .count();

    if let Some(reason) = orders.iter().find_map(|row| match row {
        OrderRow::Supported(_) => None,
        OrderRow::Unsupported(refused) | OrderRow::Ambiguous(refused) => Some(refused.reason.as_str()),
    }) {
        bail!("Orders response has a row this build cannot interpret ({reason}); reconcile current orders before changing listings.");
    }
    Ok(DecodedOrders {
        orders,
        unsupported,
        ambiguous,
    })
}

/// Whether `data` is the bucketed shape. Both buckets have to be present and
/// hold row lists: an orders response that lost one of them is not evidence
/// that the account has no orders on that side.
fn is_buckets(data: &serde_json::Value) -> bool {
    data.is_object()
        && ["sell", "buy"]
            .iter()
            .all(|bucket| data.get(bucket).is_some_and(|rows| rows.is_array()))
}

fn side_from(bucket: &str) -> Option<OrderSide> {
    match bucket {
        "sell" => Some(OrderSide::Sell),
        "buy" => Some(OrderSide::Buy),
        _ => None,
    }
}

fn unfilled(row: &serde_json::Value, reason: &str) -> OrderRow {
    OrderRow::Unsupported(RefusedRow {
        evidence: evidence(row),
        reason: reason.to_string(),
    })
}

fn decode_row(
    row: &serde_json::Value,
    bucket: Option<OrderSide>,
    constraints: &dyn ItemConstraints,
) -> OrderRow {
    let Some(id) = non_empty_str(row, "id") else {
        return unfilled(row, "row has no order id");
    };
    let Some(item_id) = non_empty_str(row, "itemId") else {
        return unfilled(row, "row has no item id");
    };

    let stated = match row.get("type").and_then(|value| value.as_str()) {
        None => None,
        Some("sell") => Some(OrderSide::Sell),
        Some("buy") => Some(OrderSide::Buy),
        Some(other) => return unfilled(row, &format!("row states an unknown side {other:?}")),
    };
    let side = match (stated, bucket) {
        (Some(stated), Some(bucket)) if stated != bucket => {
            return OrderRow::Ambiguous(RefusedRow {
                evidence: evidence(row),
                reason: format!(
                    "row is filed under {} but states {}",
                    bucket.as_str(),
                    stated.as_str()
                ),
            })
        }
        (Some(stated), _) => stated,
        (None, Some(bucket)) => bucket,
        (None, None) => return unfilled(row, "row states no side and sits in no bucket"),
    };

    let (Some(platinum), Some(quantity)) = (positive(row, "platinum"), positive(row, "quantity"))
    else {
        return unfilled(row, "row has no positive platinum or quantity");
    };

    let rank = match row.get("rank") {
        None => None,
        Some(value) if value.is_null() => None,
        Some(value) => match value.as_u64() {
            Some(rank) => Some(rank),
            None => return unfilled(row, "row carries a rank that is not a number"),
        },
    };
    let subtype = match row.get("subtype") {
        None => None,
        Some(value) if value.is_null() => None,
        Some(_) => match non_empty_str(row, "subtype") {
            Some(subtype) => Some(subtype.to_string()),
            None => return unfilled(row, "row carries an empty or non-text subtype"),
        },
    };

    if !constraints.accepts(item_id, rank, subtype.as_deref()) {
        return unfilled(
            row,
            "row's rank or subtype is not one the catalogue allows for this item",
        );
    }

    let per_trade = match row.get("perTrade") {
        None => None,
        Some(value) if value.is_null() => None,
        Some(value) => match value.as_u64() {
            Some(lot) if (1..=6).contains(&lot) && quantity % lot == 0 => Some(lot),
            _ => return unfilled(row, "row carries an invalid trade lot"),
        },
    };

    OrderRow::Supported(NormalizedOrder {
        id: id.to_string(),
        item_id: item_id.to_string(),
        side,
        platinum,
        quantity,
        per_trade,
        visible: row.get("visible").and_then(|value| value.as_bool()),
        rank,
        subtype,
    })
}

fn non_empty_str<'a>(row: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    row.get(key)
        .and_then(|value| value.as_str())
        .filter(|value| !value.is_empty())
}

fn positive(row: &serde_json::Value, key: &str) -> Option<u64> {
    row.get(key)
        .and_then(|value| value.as_u64())
        .filter(|value| *value > 0)
}

fn evidence(row: &serde_json::Value) -> serde_json::Value {
    let text = row.to_string();
    if text.len() <= MAX_EVIDENCE {
        return row.clone();
    }
    let mut end = MAX_EVIDENCE;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    serde_json::Value::String(text.get(..end).unwrap_or_default().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use market_domain::orders::OrderRow;

    /// No catalogue: the decoder's own rules without an item snapshot in play.
    struct NoCatalogue;

    impl ItemConstraints for NoCatalogue {
        fn accepts(&self, _item_id: &str, _rank: Option<u64>, _subtype: Option<&str>) -> bool {
            true
        }
    }

    /// A catalogue whose one item is a ranked mod with variants.
    struct Constrained;

    impl ItemConstraints for Constrained {
        fn accepts(&self, item_id: &str, rank: Option<u64>, subtype: Option<&str>) -> bool {
            match item_id {
                "mod" => rank.is_some_and(|rank| rank <= 5),
                "relic" => subtype == Some("radiant"),
                _ => true,
            }
        }
    }

    fn row(orders: &DecodedOrders, index: usize) -> NormalizedOrder {
        match orders.orders.get(index) {
            Some(OrderRow::Supported(order)) => order.clone(),
            other => panic!("expected a supported row at {index}, got {other:?}"),
        }
    }

    #[test]
    fn both_response_shapes_decode_to_the_same_side() {
        let bucketed = serde_json::json!({"data": {
            "sell": [{"id": "s", "itemId": "item", "platinum": 20, "quantity": 3, "rank": 0}],
            "buy": [{"id": "b", "itemId": "item", "platinum": 5, "quantity": 1}]
        }});
        let decoded = decode_orders(&bucketed, &NoCatalogue).unwrap();
        assert_eq!(decoded.orders.len(), 2);
        assert_eq!(row(&decoded, 0).side, OrderSide::Sell);
        assert_eq!(row(&decoded, 0).per_trade, None);
        assert_eq!(row(&decoded, 1).side, OrderSide::Buy);
        assert_eq!(row(&decoded, 1).rank, None);

        let flat = serde_json::json!({"data": [
            {"id": "f", "itemId": "item", "type": "sell", "platinum": 9, "quantity": 2,
             "subtype": "radiant", "perTrade": 1, "visible": true}
        ]});
        let decoded = decode_orders(&flat, &NoCatalogue).unwrap();
        let order = row(&decoded, 0);
        assert_eq!(order.side, OrderSide::Sell);
        assert_eq!(order.per_trade, Some(1));
        assert_eq!(order.visible, Some(true));
        assert_eq!(order.subtype.as_deref(), Some("radiant"));
    }

    /// A row may take its side from its bucket or state it explicitly, but the
    /// two disagreeing is the response contradicting itself: the row is a sell
    /// to a reader that trusts the bucket and a buy to one that trusts the
    /// field, so neither reading is safe.
    #[test]
    fn a_row_whose_stated_side_disagrees_with_its_bucket_is_ambiguous() {
        let body = serde_json::json!({"data": {"sell": [
            {"id": "o", "itemId": "item", "type": "buy", "platinum": 10, "quantity": 1}
        ], "buy": []}});
        let error = decode_orders(&body, &NoCatalogue).unwrap_err().to_string();
        assert!(error.contains("filed under sell but states buy"), "{error}");
    }

    #[test]
    fn a_row_that_cannot_be_read_whole_is_not_a_supported_order() {
        for (row, expected) in [
            (
                serde_json::json!({"itemId": "i", "type": "sell", "platinum": 1, "quantity": 1}),
                "no order id",
            ),
            (
                serde_json::json!({"id": "o", "type": "sell", "platinum": 1, "quantity": 1}),
                "no item id",
            ),
            (
                serde_json::json!({"id": "o", "itemId": "i", "type": "auction", "platinum": 1, "quantity": 1}),
                "unknown side",
            ),
            (
                serde_json::json!({"id": "o", "itemId": "i", "type": "sell", "quantity": 1}),
                "no positive platinum",
            ),
            (
                serde_json::json!({"id": "o", "itemId": "i", "type": "sell", "platinum": 1, "quantity": 0}),
                "no positive platinum",
            ),
            (
                serde_json::json!({"id": "o", "itemId": "i", "type": "sell", "platinum": 1, "quantity": 1, "rank": "high"}),
                "not a number",
            ),
            (
                serde_json::json!({"id": "o", "itemId": "i", "type": "sell", "platinum": 1, "quantity": 1, "subtype": ""}),
                "subtype",
            ),
            (
                serde_json::json!({"id": "o", "itemId": "i", "type": "sell", "platinum": 1, "quantity": 7, "perTrade": 2}),
                "trade lot",
            ),
            (
                serde_json::json!({"id": "o", "itemId": "i", "type": "sell", "platinum": 1, "quantity": 1, "perTrade": 0}),
                "trade lot",
            ),
            (
                serde_json::json!({"id": "o", "itemId": "i", "type": "sell", "platinum": 1, "quantity": 12, "perTrade": 12}),
                "trade lot",
            ),
        ] {
            let body = serde_json::json!({"data": {"sell": [row], "buy": []}});
            let error = decode_orders(&body, &NoCatalogue).unwrap_err().to_string();
            assert!(
                error.contains(expected),
                "expected {expected:?} for row {row}, got {error}"
            );
        }
    }

    /// An unreadable response must never reconcile as an account with no
    /// orders, so every shape the endpoint has not been observed to send is
    /// refused rather than read as empty.
    #[test]
    fn a_response_that_is_not_an_orders_list_is_refused() {
        for body in [
            serde_json::json!({}),
            serde_json::json!({"data": null}),
            serde_json::json!({"data": {"sell": []}}),
            serde_json::json!({"data": {"sell": "none"}}),
            serde_json::json!({"data": 7}),
        ] {
            assert!(decode_orders(&body, &NoCatalogue).is_err(), "{body}");
        }
        let empty = decode_orders(
            &serde_json::json!({"data": {"sell": [], "buy": []}}),
            &NoCatalogue,
        )
        .unwrap();
        assert!(empty.orders.is_empty());
        assert_eq!(empty.unsupported, 0);
    }

    /// The catalogue's rank and variant rules reach the decode through the
    /// constraints, so a row contradicting the snapshot is refused instead of
    /// being carried as an order that does not exist.
    #[test]
    fn a_row_the_catalogue_contradicts_is_refused() {
        let unranked = serde_json::json!({"data": {"sell": [
            {"id": "o", "itemId": "mod", "platinum": 1, "quantity": 1}
        ], "buy": []}});
        assert!(decode_orders(&unranked, &Constrained).is_err());

        let over_max = serde_json::json!({"data": {"sell": [
            {"id": "o", "itemId": "mod", "platinum": 1, "quantity": 1, "rank": 6}
        ], "buy": []}});
        assert!(decode_orders(&over_max, &Constrained).is_err());

        let missing_variant = serde_json::json!({"data": {"sell": [
            {"id": "o", "itemId": "relic", "platinum": 1, "quantity": 1, "subtype": "intact"}
        ], "buy": []}});
        assert!(decode_orders(&missing_variant, &Constrained).is_err());

        let valid = serde_json::json!({"data": {"sell": [
            {"id": "o", "itemId": "mod", "platinum": 1, "quantity": 1, "rank": 5},
            {"id": "p", "itemId": "relic", "platinum": 1, "quantity": 1, "subtype": "radiant"}
        ], "buy": []}});
        let decoded = decode_orders(&valid, &Constrained).unwrap();
        assert_eq!(decoded.orders.len(), 2);
        assert_eq!(row(&decoded, 1).subtype.as_deref(), Some("radiant"));
    }

    /// An item the snapshot does not know constrains nothing. The snapshot can
    /// lag the live market, and refusing an order over a missing entry would
    /// hide a real listing from the account.
    #[test]
    fn an_item_missing_from_the_catalogue_still_decodes() {
        let body = serde_json::json!({"data": [
            {"id": "o", "itemId": "unlisted", "type": "buy", "platinum": 4, "quantity": 2, "rank": 99}
        ]});
        let decoded = decode_orders(&body, &Constrained).unwrap();
        assert_eq!(row(&decoded, 0).rank, Some(99));
    }

    /// The evidence a refused row carries is what a surface has to show, so it
    /// must be bounded without being lost.
    #[test]
    fn evidence_is_kept_whole_until_it_is_oversized() {
        let small = serde_json::json!({"id": "o"});
        assert_eq!(evidence(&small), small);

        let oversized = serde_json::json!({"id": "o", "note": "x".repeat(MAX_EVIDENCE * 2)});
        match evidence(&oversized) {
            serde_json::Value::String(text) => {
                assert!(text.len() <= MAX_EVIDENCE);
                assert!(text.is_char_boundary(text.len()));
            }
            other => panic!("expected truncated text, got {other}"),
        }
    }
}
