// Derived from Rust wire contracts. Run the binding export test to update.

export type Advice = "sell_now" | "hold" | "neutral";

export type AvailableTag = { tag: string, count: number, group: TagGroup, };

export type BaroPhase = { phase: VisitPhase,
/**
 * Time left in this phase.
 */
windowMs: number | null, };

export type BaroVerdict = "flip" | "hold" | "thin" | "skip" | "unpriced";

export type BasketRequest = { rows: Array<BasketRow>,
/**
 * Ducats scrapping every spare part would yield.
 */
scrap_potential: JsNumber, };

export type BasketRow = { verdict: BaroVerdict, ducats: number | null, baseline: number | null, price: number | null, };

export type DiffOwnedRequest = {
/**
 * The last scan's `[key, count]` pairs; `null` when there is none.
 */
previous: Array<[string, JsNumber]> | null, current: Array<[string, JsNumber]>, };

export type DucatBasket = {
/**
 * Ducats the worthwhile items cost, all together.
 */
needed: number,
/**
 * How many items that is.
 */
count: number,
/**
 * How many of them scrapping every spare part would cover, best value first.
 */
coveredByScrapping: number,
/**
 * What the basket resells for at 90-day baselines.
 */
resale: number, };

export type EmptyKind = "no-market" | "price" | "owned" | "type" | "kept" | "tag" | "vault" | "ducats" | "vol" | "median" | "spares" | "advice";

export type EmptyReason = { kind: EmptyKind, excluded?: number, candidates: number, };

export type ItemDirection = "given" | "received";

export type JsNumber = number | "NaN" | "Infinity" | "-Infinity";

export type LedgerItem = { name: string, qty: number, direction: ItemDirection, };

export type LedgerSinceRequest = {
/**
 * Each trade's time, Unix seconds.
 */
at: Array<number>, days: number, now_secs: number, };

export type LedgerTotals = { sales: number, purchases: number,
/**
 * Plat received on sales.
 */
platIn: number,
/**
 * Plat spent on purchases.
 */
platOut: number, net: number, };

export type LedgerTrade = { kind: TradeKind, plat: number, items: Array<LedgerItem>, };

export type OtherSellers = { items: number, plat: number, };

export type PhaseRequest = {
/**
 * Arrival and departure in epoch ms; `NaN` when unknown.
 */
activation_ms: JsNumber, expiry_ms: JsNumber, now_ms: JsNumber, };

export type PickRow = { sellable: number, patience: boolean,
/**
 * Timing says the price sits near its 90-day low.
 */
hold: boolean, score: number, };

export type PicksRequest = { rows: Array<PickRow>,
/**
 * Defaults to [`sell_priority::MAX_PICKS`].
 */
cap?: number, };

export type SellFilterFact = { sellable: JsNumber, sell_score: JsNumber, timing: Timing, };

export type SellFilterRequest = { tags: Array<string>, rows: Array<SellFilterRow>, filters: SellFilters, };

export type SellFilterRow = { count: JsNumber, type: string | null,
/**
 * `null` is no individual copy at all, which always shows.
 */
kept_lvl: JsNumber | null, leveled: JsNumber, subtype: string | null, avg: JsNumber, vol: JsNumber, median_90d: JsNumber, low5_avg: JsNumber, ducats: JsNumber | null,
/**
 * Indices into the request's `tags`.
 */
tags: Array<number>, vault: string | null, advice: Advice | null,
/**
 * Absent when the native scorer has not answered for the row.
 */
fact?: SellFilterFact,
/**
 * The protected allocation capping the fact's quantity; absent is uncapped.
 */
allocation?: JsNumber, };

export type SellFilters = { min_price: JsNumber, min_owned: JsNumber, type_filter: string, hide_at_lvl: JsNumber, active_tags: Array<string>, vault_only: boolean, ducats_only: boolean, min_vol: JsNumber, min_median: JsNumber,
/**
 * Any of these row types; empty restricts nothing.
 */
types_any: Array<string>, spares_only: boolean,
/**
 * Only rows the advisor has a verdict for.
 */
advice_only: boolean, };

export type SellResult = { index: number, sellable: number, potential_plat: number, raw_value: number, timing: Timing, };

export type SoldItemTotal = { name: string, qty: number,
/**
 * Plat attributed to this item: whole-trade plat when it was the only
 * item given, else split evenly by quantity across the given items - an
 * honest approximation, labelled as such in the UI.
 */
plat: number, trades: number, };

export type TagGroup = "type" | "slot" | "rarity" | "era" | "companion" | "name";

export type Timing = "hold" | "drop" | "peak" | "neutral";

export type TopSellers = {
/**
 * How many of the ranked sellers to show.
 */
top: number, other: OtherSellers | null, };

export type TopSellersRequest = {
/**
 * The ranked sellers' plat, best first.
 */
plats: Array<number>, count: number, };

export type TopTagsRequest = {
/**
 * `[tag, count]`, most common first.
 */
available: Array<[string, number]>,
/**
 * Defaults to [`tags::TOP_TAGS`].
 */
limit?: JsNumber, };

export type TradeKind = "sale" | "purchase" | "trade";

export type TradeSplit = {
/**
 * Rows before this index fit today.
 */
cut: number, used: number, laterTrades: number, };

export type TradeSplitRequest = {
/**
 * Each row's trades, best row first.
 */
trades: Array<JsNumber>,
/**
 * Trades left today; `null` (or not finite) is unknown.
 */
remaining: JsNumber | null, };

export type VisitPhase = "here" | "incoming" | "unknown";

export type WebRequest = { "op": "select_picks", "input": PicksRequest } | { "op": "sell_results", "input": SellFilterRequest } | { "op": "sell_tags", "input": SellFilterRequest } | { "op": "sell_empty_reason", "input": SellFilterRequest } | { "op": "tag_groups", "input": Array<string> } | { "op": "top_tags", "input": TopTagsRequest } | { "op": "split_by_trades", "input": TradeSplitRequest } | { "op": "ledger_totals", "input": Array<LedgerTrade> } | { "op": "ledger_since", "input": LedgerSinceRequest } | { "op": "sold_by_item", "input": Array<LedgerTrade> } | { "op": "top_sellers", "input": TopSellersRequest } | { "op": "median_week", "input": Array<WeekPlat> } | { "op": "ducat_basket", "input": BasketRequest } | { "op": "baro_phase", "input": PhaseRequest } | { "op": "diff_owned", "input": DiffOwnedRequest };

export type WebResponse = { "op": "select_picks", "result": Array<number> } | { "op": "sell_results", "result": Array<SellResult> } | { "op": "sell_tags", "result": Array<AvailableTag> } | { "op": "sell_empty_reason", "result": EmptyReason } | { "op": "tag_groups", "result": Array<TagGroup> } | { "op": "top_tags", "result": Array<string> } | { "op": "split_by_trades", "result": TradeSplit } | { "op": "ledger_totals", "result": LedgerTotals } | { "op": "ledger_since", "result": Array<number> } | { "op": "sold_by_item", "result": Array<SoldItemTotal> } | { "op": "top_sellers", "result": TopSellers } | { "op": "median_week", "result": number | null } | { "op": "ducat_basket", "result": DucatBasket } | { "op": "baro_phase", "result": BaroPhase } | { "op": "diff_owned", "result": Array<[string, number]> };

export type WeekPlat = { plat: number,
/**
 * The current week, still running.
 */
partial: boolean, };
