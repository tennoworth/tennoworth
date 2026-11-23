//! Rules the views apply synchronously over snapshot objects, answered by
//! `market-domain-web` (the wasm32 module the site and the desktop webview
//! load before they render). Rules over plain numbers skip this: that crate
//! exports them as functions of numbers.
//!
//! Ops gated on `desktop-views` serve desktop-only views; the hosted site's
//! build leaves them out (and with them their readers), so the module every
//! visitor downloads carries only what the site calls.
#![cfg_attr(not(feature = "desktop-views"), allow(unused_imports))]

use crate::buyers::{self, BuyerCompareRequest, BuyerComparison};
use crate::calendar;
use crate::listing::{
    self, BatchRequest, DriftRequest, DriftRow, HealthReport, HealthRequest, InterruptedBatch,
    LiveAskRead, LiveAskRequest, PriceRuleRequest,
};
use crate::math::sell_priority::{self, PickCandidate};
use crate::rivens::appraise::{Appraisal, AppraiseRequest, CompsSampleRead, DescribeCompsRequest};
use crate::rivens::{
    self, ExtractRivensRequest, OwnedRiven, ResolveRivensRequest, RivenRow, RivenRowsRequest,
    RivenSimilarityRequest,
};
use crate::{baro, inventory, ledger, sell_filter, tags, trade_session};
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
    ($($(#[$gate:meta])* $name:ident: $input:ty => $output:ty = $run:expr),+ $(,)?) => {
        #[derive(TS)]
        #[ts(tag = "op", content = "input", rename_all = "snake_case")]
        pub enum WebRequest { $($(#[$gate])* $name($input)),+ }

        // The derived adjacently tagged form compiles every input's
        // deserializer twice (once to buffer an `input` that arrives before
        // `op`), which doubled the module the views download. Reading `input`
        // raw and parsing it once its op is known compiles each one once.
        impl<'de> Deserialize<'de> for WebRequest {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                #[derive(Deserialize)]
                #[serde(rename_all = "snake_case")]
                enum Op { $($(#[$gate])* $name),+ }
                #[derive(Deserialize)]
                struct Envelope<'a> {
                    op: Op,
                    #[serde(borrow)]
                    input: &'a serde_json::value::RawValue,
                }
                let Envelope { op, input } = Envelope::deserialize(d)?;
                match op { $($(#[$gate])* Op::$name => serde_json::from_str(input.get()).map(Self::$name)),+ }
                    .map_err(serde::de::Error::custom)
            }
        }

        #[derive(Serialize, TS)]
        #[serde(tag = "op", content = "result", rename_all = "snake_case")]
        pub enum WebResponse { $($(#[$gate])* $name($output)),+ }

        impl WebRequest {
            pub fn execute(self) -> Result<WebResponse, String> {
                match self { $($(#[$gate])* Self::$name(input) => ($run)(input).map(WebResponse::$name)),+ }
            }
        }
    };
}

web_operations! {
    #[cfg(feature = "desktop-views")]
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
    #[cfg(feature = "desktop-views")]
    FindBargains: BargainsRequest => Vec<BargainLead> = |input: BargainsRequest| Ok::<_, String>(browse::find_bargains(&input)),
    MetaDrift: MetaDriftRequest => Option<MetaDrift> = |input: MetaDriftRequest| meta_drift::meta_drift(&input),
    YearViews: YearViewsRequest => Vec<YearView> = |input: YearViewsRequest| Ok::<_, String>(advisor::year_views(&input)),
    #[cfg(feature = "desktop-views")]
    PriceFromRule: PriceRuleRequest => Option<f64> = |input: PriceRuleRequest| Ok::<_, String>(
        listing::price_from_rule(input.rule, &input.evidence)
    ),
    #[cfg(feature = "desktop-views")]
    ReasonableLowestAsk: LiveAskRequest => LiveAskRead = |input: LiveAskRequest| Ok::<_, String>(
        listing::reasonable_lowest_ask(&input.asks, input.median)
    ),
    #[cfg(feature = "desktop-views")]
    AssessListings: HealthRequest => HealthReport = |input: HealthRequest| Ok::<_, String>(
        listing::assess_listings(&input.orders, &input.prefs)
    ),
    #[cfg(feature = "desktop-views")]
    SelectDrifted: DriftRequest => Vec<DriftRow> = |input: DriftRequest| Ok::<_, String>(
        listing::select_drifted(&input.orders, input.limit.unwrap_or(listing::DRIFT_LIMIT))
    ),
    #[cfg(feature = "desktop-views")]
    InterruptedBatch: BatchRequest => Option<InterruptedBatch> = |input: BatchRequest| Ok::<_, String>(
        listing::interrupted_batch(&input.statuses)
    ),
    #[cfg(feature = "desktop-views")]
    CompareBuyers: BuyerCompareRequest => BuyerComparison = |input: BuyerCompareRequest| Ok::<_, String>(
        buyers::compare_buyers(&input)
    ),
    CalendarFeed: calendar::CalendarRequest => Vec<calendar::feed::FeedItem> = |input: calendar::CalendarRequest| Ok::<_, String>(
        calendar::feed::build(&input.market, input.owned.as_deref(), input.now)
    ),
    CalendarEvents: calendar::CalendarRequest => Vec<calendar::events::DatedEvent> = |input: calendar::CalendarRequest| Ok::<_, String>(
        calendar::events::build(&input.market, &input.feed, input.owned.as_deref(), input.now)
    ),
    #[cfg(feature = "desktop-views")]
    SellResults: sell_filter::SellFilterRequest => Vec<sell_filter::SellResult> = |input: sell_filter::SellFilterRequest| Ok::<_, String>(
        sell_filter::sell_results(&input)
    ),
    #[cfg(feature = "desktop-views")]
    SellTags: sell_filter::SellFilterRequest => Vec<sell_filter::AvailableTag> = |input: sell_filter::SellFilterRequest| Ok::<_, String>(
        sell_filter::available_tags(&input)
    ),
    #[cfg(feature = "desktop-views")]
    SellEmptyReason: sell_filter::SellFilterRequest => sell_filter::EmptyReason = |input: sell_filter::SellFilterRequest| Ok::<_, String>(
        sell_filter::empty_reason(&input)
    ),
    #[cfg(feature = "desktop-views")]
    TagGroups: Vec<String> => Vec<tags::TagGroup> = |input: Vec<String>| Ok::<_, String>(
        input.iter().map(|tag| tags::tag_group(tag)).collect()
    ),
    #[cfg(feature = "desktop-views")]
    TopTags: TopTagsRequest => Vec<String> = |input: TopTagsRequest| Ok::<_, String>(
        tags::top_tags(&input.available, input.limit.map_or(tags::TOP_TAGS, |n| n.0))
    ),
    #[cfg(feature = "desktop-views")]
    SplitByTrades: trade_session::TradeSplitRequest => trade_session::TradeSplit = |input: trade_session::TradeSplitRequest| Ok::<_, String>(
        trade_session::split_by_trades(&input)
    ),
    #[cfg(feature = "desktop-views")]
    LedgerTotals: Vec<ledger::LedgerTrade> => ledger::LedgerTotals = |input: Vec<ledger::LedgerTrade>| Ok::<_, String>(
        ledger::totals(&input)
    ),
    #[cfg(feature = "desktop-views")]
    LedgerSince: ledger::LedgerSinceRequest => Vec<usize> = |input: ledger::LedgerSinceRequest| Ok::<_, String>(
        ledger::since(&input)
    ),
    #[cfg(feature = "desktop-views")]
    SoldByItem: Vec<ledger::LedgerTrade> => Vec<ledger::SoldItemTotal> = |input: Vec<ledger::LedgerTrade>| Ok::<_, String>(
        ledger::sold_by_item(&input)
    ),
    #[cfg(feature = "desktop-views")]
    TopSellers: ledger::TopSellersRequest => ledger::TopSellers = |input: ledger::TopSellersRequest| Ok::<_, String>(
        ledger::top_sellers(&input)
    ),
    #[cfg(feature = "desktop-views")]
    MedianWeek: Vec<ledger::WeekPlat> => Option<f64> = |input: Vec<ledger::WeekPlat>| Ok::<_, String>(
        ledger::median_week(&input)
    ),
    #[cfg(feature = "desktop-views")]
    DucatBasket: baro::BasketRequest => baro::DucatBasket = |input: baro::BasketRequest| Ok::<_, String>(
        baro::ducat_basket(&input)
    ),
    BaroPhase: baro::PhaseRequest => baro::BaroPhase = |input: baro::PhaseRequest| Ok::<_, String>(
        baro::baro_phase(&input)
    ),
    #[cfg(feature = "desktop-views")]
    DiffOwned: inventory::DiffOwnedRequest => Vec<(String, f64)> = |input: inventory::DiffOwnedRequest| Ok::<_, String>(
        inventory::diff_owned(&input)
    ),
    #[cfg(feature = "desktop-views")]
    ExtractRivens: ExtractRivensRequest => Vec<OwnedRiven> =
        |input: ExtractRivensRequest| Ok::<_, String>(rivens::extract_rivens(&input.upgrades)),
    #[cfg(feature = "desktop-views")]
    ResolveRivens: ResolveRivensRequest => Vec<Option<usize>> =
        |input: ResolveRivensRequest| Ok::<_, String>(rivens::resolve_rivens(&input.rivens, &input.game_refs)),
    #[cfg(feature = "desktop-views")]
    RivenRows: RivenRowsRequest => Vec<RivenRow> = |input: RivenRowsRequest| Ok::<_, String>(rivens::riven_rows(&input)),
    #[cfg(feature = "desktop-views")]
    RivenSimilarity: RivenSimilarityRequest => Vec<Option<f64>> =
        |input: RivenSimilarityRequest| Ok::<_, String>(rivens::riven_similarities(&input)),
    #[cfg(feature = "desktop-views")]
    SplicedTraits: () => Vec<&'static str> = |(): ()| Ok::<_, String>(rivens::spliced_traits()),
    #[cfg(feature = "desktop-views")]
    AppraiseRiven: AppraiseRequest => Appraisal = |input: AppraiseRequest| Ok::<_, String>(input.run()),
    #[cfg(feature = "desktop-views")]
    DescribeRivenComps: DescribeCompsRequest => CompsSampleRead = |input: DescribeCompsRequest| {
        Ok::<_, String>(rivens::appraise::describe_comps(&input.comps, input.now_ms))
    },
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
