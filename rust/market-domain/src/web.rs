//! Rules the views apply synchronously over snapshot objects, answered by
//! `market-domain-web` (the wasm32 module the site and the desktop webview
//! load before they render). Rules over plain numbers skip this: that crate
//! exports them as functions of numbers.

use crate::math::sell_priority::{self, PickCandidate};
use crate::{baro, inventory, ledger, sell_filter, tags, trade_session};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// One row as the picks rule reads it.
#[derive(Debug, Clone, Copy, Deserialize, TS)]
pub struct PickRow {
    pub sellable: f64,
    pub patience: bool,
    /// Timing says the price sits near its 90-day low.
    pub hold: bool,
    pub score: f64,
}

impl From<PickRow> for PickCandidate {
    fn from(r: PickRow) -> Self {
        PickCandidate {
            sellable: r.sellable,
            patience: r.patience,
            hold: r.hold,
            score: r.score,
        }
    }
}

#[derive(Debug, Clone, Deserialize, TS)]
pub struct PicksRequest {
    pub rows: Vec<PickRow>,
    /// Defaults to [`sell_priority::MAX_PICKS`].
    #[serde(default)]
    #[ts(optional)]
    pub cap: Option<usize>,
}

#[derive(Debug, Clone, Deserialize, TS)]
pub struct TopTagsRequest {
    /// `[tag, count]`, most common first.
    pub available: Vec<(String, f64)>,
    /// Defaults to [`tags::TOP_TAGS`].
    #[serde(default)]
    #[ts(optional)]
    pub limit: Option<crate::js::JsNumber>,
}

macro_rules! web_operations {
    ($($name:ident: $input:ty => $output:ty = $run:expr),+ $(,)?) => {
        #[derive(Deserialize, TS)]
        #[serde(tag = "op", content = "input", rename_all = "snake_case")]
        pub enum WebRequest { $($name($input)),+ }

        #[derive(Serialize, TS)]
        #[serde(tag = "op", content = "result", rename_all = "snake_case")]
        pub enum WebResponse { $($name($output)),+ }

        impl WebRequest {
            pub fn execute(self) -> Result<WebResponse, String> {
                match self { $(Self::$name(input) => ($run)(input).map(WebResponse::$name)),+ }
            }
        }
    };
}

web_operations! {
    SelectPicks: PicksRequest => Vec<usize> = |input: PicksRequest| Ok::<_, String>(
        sell_priority::select_picks(
            &input.rows.into_iter().map(PickCandidate::from).collect::<Vec<_>>(),
            input.cap.unwrap_or(sell_priority::MAX_PICKS),
        )
    ),
    SellResults: sell_filter::SellFilterRequest => Vec<sell_filter::SellResult> = |input: sell_filter::SellFilterRequest| Ok::<_, String>(
        sell_filter::sell_results(&input)
    ),
    SellTags: sell_filter::SellFilterRequest => Vec<sell_filter::AvailableTag> = |input: sell_filter::SellFilterRequest| Ok::<_, String>(
        sell_filter::available_tags(&input)
    ),
    SellEmptyReason: sell_filter::SellFilterRequest => sell_filter::EmptyReason = |input: sell_filter::SellFilterRequest| Ok::<_, String>(
        sell_filter::empty_reason(&input)
    ),
    TagGroups: Vec<String> => Vec<tags::TagGroup> = |input: Vec<String>| Ok::<_, String>(
        input.iter().map(|tag| tags::tag_group(tag)).collect()
    ),
    TopTags: TopTagsRequest => Vec<String> = |input: TopTagsRequest| Ok::<_, String>(
        tags::top_tags(&input.available, input.limit.map_or(tags::TOP_TAGS, |n| n.0))
    ),
    SplitByTrades: trade_session::TradeSplitRequest => trade_session::TradeSplit = |input: trade_session::TradeSplitRequest| Ok::<_, String>(
        trade_session::split_by_trades(&input)
    ),
    LedgerTotals: Vec<ledger::LedgerTrade> => ledger::LedgerTotals = |input: Vec<ledger::LedgerTrade>| Ok::<_, String>(
        ledger::totals(&input)
    ),
    LedgerSince: ledger::LedgerSinceRequest => Vec<usize> = |input: ledger::LedgerSinceRequest| Ok::<_, String>(
        ledger::since(&input)
    ),
    SoldByItem: Vec<ledger::LedgerTrade> => Vec<ledger::SoldItemTotal> = |input: Vec<ledger::LedgerTrade>| Ok::<_, String>(
        ledger::sold_by_item(&input)
    ),
    TopSellers: ledger::TopSellersRequest => ledger::TopSellers = |input: ledger::TopSellersRequest| Ok::<_, String>(
        ledger::top_sellers(&input)
    ),
    MedianWeek: Vec<ledger::WeekPlat> => Option<f64> = |input: Vec<ledger::WeekPlat>| Ok::<_, String>(
        ledger::median_week(&input)
    ),
    DucatBasket: baro::BasketRequest => baro::DucatBasket = |input: baro::BasketRequest| Ok::<_, String>(
        baro::ducat_basket(&input)
    ),
    BaroPhase: baro::PhaseRequest => baro::BaroPhase = |input: baro::PhaseRequest| Ok::<_, String>(
        baro::baro_phase(&input)
    ),
    DiffOwned: inventory::DiffOwnedRequest => Vec<(String, f64)> = |input: inventory::DiffOwnedRequest| Ok::<_, String>(
        inventory::diff_owned(&input)
    ),
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn call(request: serde_json::Value) -> serde_json::Value {
        let request: WebRequest = serde_json::from_value(request).unwrap();
        serde_json::to_value(request.execute().unwrap()).unwrap()
    }

    #[test]
    fn picks_keep_the_callers_order_and_cap() {
        let row = |score: f64, hold: bool| json!({ "sellable": 1, "patience": false, "hold": hold, "score": score });
        let rows: Vec<_> = (0..8).map(|i| row(f64::from(90 - i), i == 1)).collect();
        assert_eq!(
            call(json!({ "op": "select_picks", "input": { "rows": rows } })),
            json!({ "op": "select_picks", "result": [0, 2, 3, 4, 5] })
        );
        assert_eq!(
            call(json!({ "op": "select_picks", "input": { "rows": rows, "cap": 2 } })),
            json!({ "op": "select_picks", "result": [0, 2] })
        );
    }

    #[test]
    fn web_bindings_match_rust() {
        let mut types = crate::bindings::TypeScript::default();
        types.add::<WebRequest>();
        types.add::<WebResponse>();
        let expected = types.finish().expect("unique web types");
        let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../frontend/src/contracts/generated/web.ts");
        if std::env::var_os("TENNOWORTH_UPDATE_BINDINGS").is_some() {
            std::fs::write(&file, &expected).expect("write web bindings");
        }
        assert_eq!(
            std::fs::read_to_string(file).expect("run TENNOWORTH_UPDATE_BINDINGS=1 cargo test -p market-domain web_bindings_match_rust"),
            expected
        );
    }
}
