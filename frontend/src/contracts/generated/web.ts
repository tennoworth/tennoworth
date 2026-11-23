// Derived from Rust wire contracts. Run the binding export test to update.

export type BatchRequest = {
/**
 * Each item's journal status; a status this build does not know counts
 * as nothing.
 */
statuses: Array<string>, };

export type BuyerBid = { id?: string | null, user_id?: string | null, status?: string | null, quantity?: number | null, per_trade?: number | null,
/**
 * Total price for one complete lot.
 */
platinum?: number | null, };

export type BuyerBookQuote = { orders?: Array<BuyerBid> | null,
/**
 * Whether the account was known, so its own bids are not in `orders`.
 */
own_orders_excluded?: boolean | null, };

export type BuyerCompareError = "invalid" | "unavailable" | "locked" | "stale";

export type BuyerCompareRequest = { quote?: BuyerQuote | null, quantity?: number | null,
/**
 * The listing's per-unit ask.
 */
ask?: number | null,
/**
 * Epoch milliseconds of the comparison and of the book's observation;
 * the page reads both clocks, so the rule never does.
 */
now?: number | null, observed?: number | null, };

export type BuyerComparison = { error: BuyerCompareError, } | BuyerCoverage;

export type BuyerCoverage = { rows: Array<BuyerFill>, covered: number, uncovered: number, value: number,
/**
 * The covered quantity at the listing's ask.
 */
ask_value: number, gap: number, };

export type BuyerFill = {
/**
 * The buyer's position in the quote's `orders`.
 */
order: number, quantity: number, value: number, };

export type BuyerQuote = {
/**
 * Set when the lookup failed.
 */
error?: string | null, buyer_book?: BuyerBookQuote | null, };

export type DriftInput = { id: string, slug: string, name: string, platinum: number,
/**
 * Only sell listings are analysed; see [`select_drifted`].
 */
type?: OrderSide | null, m?: DriftMarket | null, };

export type DriftKind = "overpriced" | "underpriced";

export type DriftMarket = { vol?: number | null, low_sell?: number | null, avg?: number | null, median_now?: number | null, median_90d?: number | null, };

export type DriftRequest = { orders: Array<DriftInput>,
/**
 * Defaults to [`DRIFT_LIMIT`].
 */
limit?: number, };

export type DriftRow = { id: string, slug: string, name: string,
/**
 * What the listing asks now.
 */
listed: number,
/**
 * What it would be repriced to.
 */
suggested: number,
/**
 * Signed difference, suggested - listed.
 */
delta: number,
/**
 * Magnitude of the move as a fraction of the listed price.
 */
delta_pct: number, kind: DriftKind,
/**
 * Below the liquidity floor the book is thin enough that the suggestion
 * is weak evidence: the view de-emphasises rather than hides, as the sell
 * picks treat the same threshold.
 */
thin: boolean, };

export type HealthInput = { id: string, slug: string, name: string,
/**
 * The per-unit price of the listing's lot.
 */
platinum: number, quantity: number,
/**
 * Only a sell listing is assessed.
 */
type: OrderSide,
/**
 * Items per trade.
 */
perTrade?: number | null,
/**
 * Snapshot 48-hour volume of the listing's rank, for the thin-book mark;
 * `null` when the snapshot does not describe that rank.
 */
vol?: number | null,
/**
 * Live top-of-book for this listing's tier; `null` = not checked.
 */
live?: LiveBook | null,
/**
 * Tradeable copies owned per the latest scan; `null` = unknown.
 */
owned?: number | null, stats?: HealthStats | null, };

export type HealthIssue = { id: string, slug: string, name: string, kind: HealthKind,
/**
 * What the listing says now: the unit price for price kinds, the
 * quantity for quantity kinds.
 */
current: number,
/**
 * What to change it to. `0` on `not-owned` means "delete". Price kinds
 * give the per-unit price of `suggestedLot`.
 */
suggested: number,
/**
 * Price kinds only: the whole-platinum lot price a reprice sends.
 */
suggestedLot?: number,
/**
 * `underpriced` only: the book is too thin for the target to be firm.
 */
thin?: boolean,
/**
 * `overpriced` only: the cheapest live ask, skipped as an outlier.
 */
ignoredAsk?: number, };

export type HealthKind = "overpriced" | "underbid" | "underpriced" | "excess-qty" | "not-owned";

export type HealthPrefs = { rule: PriceRule,
/**
 * Platinum an ask may sit above the rule's target before it is flagged.
 */
tolerance: number,
/**
 * A kind this build does not know mutes nothing.
 */
muted: Array<HealthKind>, };

export type HealthReport = { issues: Array<HealthIssue>, summary: HealthSummary, };

export type HealthRequest = { orders: Array<HealthInput>, prefs: HealthPrefs, };

export type HealthStats = {
/**
 * A key that is present stands even when it is `null`; only an absent one
 * lets the live asks stand in, on a rank the snapshot does not describe.
 */
low5Avg?: number | null, low20Avg?: number | null, avg48h?: number | null, median90d?: number | null,
/**
 * Today's median, else the 90-day baseline: what the live-ask read
 * judges outliers against.
 */
median?: number | null, };

export type HealthSummary = { overpriced: number, underbid: number, underpriced: number, excessQty: number, notOwned: number, total: number, };

export type InterruptedBatch = { pending: number, uncertain: number, done: number,
/**
 * Whether Resume may be offered: unsent work, or an unknown outcome a
 * check of current orders may resolve.
 */
resumable: boolean, };

export type LiveAskRead = {
/**
 * The cheapest ask worth competing with, or `null` with no asks.
 */
ask: number | null,
/**
 * The cheapest ask, when it was skipped as an outlier.
 */
ignored: number | null, };

export type LiveAskRequest = { asks: Array<number | null>, median?: number | null, };

export type LiveBook = {
/**
 * The best online asks, cheapest first, the listing's own excluded.
 */
sells?: Array<number | null> | null, low_sell?: number | null, top_buy?: number | null,
/**
 * Set when this lookup failed; the row has no live data.
 */
error?: string | null, };

export type OrderSide = "sell" | "buy";

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

export type PriceBasis = "lowest-ask" | "low5-avg" | "low20-avg" | "avg-48h" | "median-90d";

export type PriceEvidence = {
/**
 * Lowest other online ask (own order excluded), or the troll-clamped
 * clearing price when no live check has run.
 */
lowestAsk?: number | null, low5Avg?: number | null,
/**
 * Snapshot only: a live check returns at most five asks.
 */
low20Avg?: number | null, avg48h?: number | null, median90d?: number | null, };

export type PriceRule = {
/**
 * `None` for a basis this build does not know, which prices nothing.
 */
basis: PriceBasis, offset: number, };

export type PriceRuleRequest = { rule: PriceRule, evidence: PriceEvidence, };

export type WebRequest = { "op": "select_picks", "input": PicksRequest } | { "op": "price_from_rule", "input": PriceRuleRequest } | { "op": "reasonable_lowest_ask", "input": LiveAskRequest } | { "op": "assess_listings", "input": HealthRequest } | { "op": "select_drifted", "input": DriftRequest } | { "op": "interrupted_batch", "input": BatchRequest } | { "op": "compare_buyers", "input": BuyerCompareRequest };

export type WebResponse = { "op": "select_picks", "result": Array<number> } | { "op": "price_from_rule", "result": number | null } | { "op": "reasonable_lowest_ask", "result": LiveAskRead } | { "op": "assess_listings", "result": HealthReport } | { "op": "select_drifted", "result": Array<DriftRow> } | { "op": "interrupted_batch", "result": InterruptedBatch | null } | { "op": "compare_buyers", "result": BuyerComparison };
