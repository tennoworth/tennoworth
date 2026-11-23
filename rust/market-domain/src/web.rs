//! Rules the views apply synchronously over snapshot objects, answered by
//! `market-domain-web` (the wasm32 module the site and the desktop webview
//! load before they render). Rules over plain numbers skip this: that crate
//! exports them as functions of numbers.

use crate::buyers::{self, BuyerCompareRequest, BuyerComparison};
use crate::listing::{
    self, BatchRequest, DriftRequest, DriftRow, HealthReport, HealthRequest, InterruptedBatch,
    LiveAskRead, LiveAskRequest, PriceRuleRequest,
};
use crate::math::sell_priority::{self, PickCandidate};
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
    PriceFromRule: PriceRuleRequest => Option<f64> = |input: PriceRuleRequest| Ok::<_, String>(
        listing::price_from_rule(input.rule, &input.evidence)
    ),
    ReasonableLowestAsk: LiveAskRequest => LiveAskRead = |input: LiveAskRequest| Ok::<_, String>(
        listing::reasonable_lowest_ask(&input.asks, input.median)
    ),
    AssessListings: HealthRequest => HealthReport = |input: HealthRequest| Ok::<_, String>(
        listing::assess_listings(&input.orders, &input.prefs)
    ),
    SelectDrifted: DriftRequest => Vec<DriftRow> = |input: DriftRequest| Ok::<_, String>(
        listing::select_drifted(&input.orders, input.limit.unwrap_or(listing::DRIFT_LIMIT))
    ),
    InterruptedBatch: BatchRequest => Option<InterruptedBatch> = |input: BatchRequest| Ok::<_, String>(
        listing::interrupted_batch(&input.statuses)
    ),
    CompareBuyers: BuyerCompareRequest => BuyerComparison = |input: BuyerCompareRequest| Ok::<_, String>(
        buyers::compare_buyers(&input)
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
