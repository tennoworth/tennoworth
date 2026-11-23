//! Rules the views apply synchronously over snapshot objects, answered by
//! `market-domain-web` (the wasm32 module the site and the desktop webview
//! load before they render). Rules over plain numbers skip this: that crate
//! exports them as functions of numbers.

use crate::math::sell_priority::{self, PickCandidate};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::advisor::{self, YearView, YearViewsRequest};
use crate::browse::{
    self, BargainLead, BargainsRequest, BrowseIndexRequest, BrowseNames, DispositionRequest,
    HandoffPick, HandoffRequest, Movers, MoversRequest, SearchRequest, VaultedRequest,
};
use crate::meta_drift::{self, MetaDrift, MetaDriftRequest};

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
        #[derive(TS)]
        #[ts(tag = "op", content = "input", rename_all = "snake_case")]
        pub enum WebRequest { $($name($input)),+ }

        // The derived adjacently tagged form compiles every input's
        // deserializer twice (once to buffer an `input` that arrives before
        // `op`), which doubled the module the views download. Reading `input`
        // raw and parsing it once its op is known compiles each one once.
        impl<'de> Deserialize<'de> for WebRequest {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                #[derive(Deserialize)]
                #[serde(rename_all = "snake_case")]
                enum Op { $($name),+ }
                #[derive(Deserialize)]
                struct Envelope<'a> {
                    op: Op,
                    #[serde(borrow)]
                    input: &'a serde_json::value::RawValue,
                }
                let Envelope { op, input } = Envelope::deserialize(d)?;
                match op { $(Op::$name => serde_json::from_str(input.get()).map(Self::$name)),+ }
                    .map_err(serde::de::Error::custom)
            }
        }

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
    BrowseIndex: BrowseIndexRequest => BrowseNames = |input| Ok::<_, String>(browse::browse_index(input)),
    SlugName: String => String = |slug: String| Ok::<_, String>(browse::slug_name(&slug)),
    SearchItems: SearchRequest => Vec<usize> = |input: SearchRequest| Ok::<_, String>(browse::search(&input)),
    TopMovers: MoversRequest => Movers = |input: MoversRequest| Ok::<_, String>(browse::top_movers(&input)),
    VaultedTop: VaultedRequest => Vec<usize> = |input: VaultedRequest| Ok::<_, String>(browse::vaulted_top(&input)),
    DispositionChanges: DispositionRequest => Vec<usize> = |input: DispositionRequest| browse::disposition_changes(&input),
    HandoffSample: HandoffRequest => Vec<HandoffPick> = |input: HandoffRequest| Ok::<_, String>(browse::handoff_sample(&input)),
    FindBargains: BargainsRequest => Vec<BargainLead> = |input: BargainsRequest| Ok::<_, String>(browse::find_bargains(&input)),
    MetaDrift: MetaDriftRequest => Option<MetaDrift> = |input: MetaDriftRequest| meta_drift::meta_drift(&input),
    YearViews: YearViewsRequest => Vec<YearView> = |input: YearViewsRequest| Ok::<_, String>(advisor::year_views(&input)),
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn call(request: serde_json::Value) -> serde_json::Value {
        let request: WebRequest = serde_json::from_str(&request.to_string()).unwrap();
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
    fn the_market_lists_answer_through_the_channel() {
        assert_eq!(
            call(
                json!({ "op": "search_items", "input": { "query": "prime", "limit": 5, "names": [["mag prime", 5], ["prime set", 1], ["prime x", null]] } })
            ),
            json!({ "op": "search_items", "result": [1, 2, 0] })
        );
        assert_eq!(
            call(json!({ "op": "slug_name", "input": "lato_prime" })),
            json!({ "op": "slug_name", "result": "Lato Prime" })
        );
        let views = call(
            json!({ "op": "year_views", "input": { "medians": [[10, null, 12]], "min_days": 0, "buckets": 1 } }),
        );
        assert_eq!(views["result"][0]["weekly"], json!([11.0]));
        assert_eq!(views["result"][0]["stats"]["latestIdx"], json!(2));
        let days: Vec<_> = (0..104).map(|d| json!(f64::from(d))).collect();
        let defaults = call(json!({ "op": "year_views", "input": { "medians": [days] } }));
        assert_eq!(
            defaults["result"][0]["weekly"].as_array().unwrap().len(),
            52
        );
        assert_eq!(defaults["result"][0]["stats"]["tradedDays"], json!(104));
        let thin: Vec<_> = (0..19).map(|d| json!(f64::from(d))).collect();
        let thin = call(json!({ "op": "year_views", "input": { "medians": [thin] } }));
        assert_eq!(thin["result"][0]["stats"], Value::Null);
        assert_eq!(
            call(json!({ "op": "meta_drift", "input": { "usage_history": null } })),
            json!({ "op": "meta_drift", "result": null })
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
