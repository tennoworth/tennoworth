//! Rules the views apply synchronously over snapshot objects, answered by
//! `market-domain-web` (the wasm32 module the site and the desktop webview
//! load before they render). Rules over plain numbers skip this: that crate
//! exports them as functions of numbers.

use crate::math::sell_priority::{self, PickCandidate};
use crate::rivens::appraise::{AppraiseRequest, Appraisal, CompsSampleRead, DescribeCompsRequest};
use crate::rivens::{self, ExtractRivensRequest, OwnedRiven, ResolveRivensRequest, RivenRow, RivenRowsRequest, RivenSimilarityRequest};
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
    ExtractRivens: ExtractRivensRequest => Vec<OwnedRiven> =
        |input: ExtractRivensRequest| Ok::<_, String>(rivens::extract_rivens(&input.upgrades)),
    ResolveRivens: ResolveRivensRequest => Vec<Option<usize>> =
        |input: ResolveRivensRequest| Ok::<_, String>(rivens::resolve_rivens(&input.rivens, &input.game_refs)),
    RivenRows: RivenRowsRequest => Vec<RivenRow> = |input: RivenRowsRequest| Ok::<_, String>(rivens::riven_rows(&input)),
    RivenSimilarity: RivenSimilarityRequest => Vec<Option<f64>> =
        |input: RivenSimilarityRequest| Ok::<_, String>(rivens::riven_similarities(&input)),
    SplicedTraits: () => Vec<&'static str> = |(): ()| Ok::<_, String>(rivens::spliced_traits()),
    AppraiseRiven: AppraiseRequest => Appraisal = |input: AppraiseRequest| Ok::<_, String>(input.run()),
    DescribeRivenComps: DescribeCompsRequest => CompsSampleRead = |input: DescribeCompsRequest| {
        Ok::<_, String>(rivens::appraise::describe_comps(&input.comps, input.now_ms))
    },
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
