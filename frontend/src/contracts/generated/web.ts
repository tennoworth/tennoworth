// Derived from Rust wire contracts. Run the binding export test to update.

export type AlertReward = { item: string, count: number, };

export type BargainItem = { ask: number, bid: number, median: number, vol: number, low5: number, };

export type BargainKind = "flip" | "cheap";

export type BargainLead = { at: number, kind: BargainKind, ask: number,
/**
 * flip: the bid that would buy it back; cheap: the 90-day median.
 */
resale: number, margin: number, vol: number, };

export type BargainsRequest = {
/**
 * Every `items` entry, in snapshot order.
 */
items: Array<BargainItem>,
/**
 * Defaults to 12.
 */
limit?: number, };

export type BatchRequest = {
/**
 * Each item's journal status; a status this build does not know counts
 * as nothing.
 */
statuses: Array<string>, };

export type Boost = "resource-amount" | "resource-chance" | "affinity" | "credits" | "other";

export type BrowseIndexRequest = {
/**
 * `catalog` entries in snapshot order: lowercased name, slug.
 */
catalog: Array<[string, string]> | null,
/**
 * `items` slugs in snapshot order, each with whether its entry is present.
 */
items: Array<[string, boolean]> | null, };

export type BrowseName = { slug: string, name: string, nameLower: string, };

export type BrowseNames = {
/**
 * One entry per priceable catalog name, for type-ahead search. Search is a
 * sell tool: an un-priceable quest key is noise.
 */
names: Array<BrowseName>,
/**
 * The label of every named catalog slug and every item slug.
 */
labels: Array<[string, string]>, };

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

export type CalendarHolding = { slug: string, count?: number | null, };

export type CalendarRequest = {
/**
 * The snapshot's `baro`, `calendar`, `set_to_parts`, `de`,
 * `event_rewards` and `surface_provenance`.
 */
market: unknown,
/**
 * The curated announcement feed, already validated by the page's
 * reader; only the month and lane calendar shows it.
 */
feed?: unknown,
/**
 * Absent before an inventory is loaded, which is not an empty one.
 */
owned?: Array<CalendarHolding> | null, now: number, };

export type DatedEvent = {
/**
 * Stable across refreshes of the same snapshot, so a selection survives.
 */
id: string, kind: DatedKind,
/**
 * Epoch ms.
 */
from: number,
/**
 * Epoch ms; none for a single moment such as a deadline.
 */
to: number | null, text: DatedText, estimate: boolean, source: DatedSource, sourceUrl?: string,
/**
 * Held slugs this event touches; absent when that cannot be known.
 */
affects?: Array<string>, };

export type DatedKind = "official" | "event" | "baro" | "resurgence" | "vault" | "notice" | "deal" | "nightwave";

export type DatedSource = "worldstate" | "estimate" | "notice" | "official";

export type DatedText = { "kind": "baro", where: string | null, } | { "kind": "baro-estimate" } | { "kind": "resurgence", sets: Array<string>, } | { "kind": "vault", sets: Array<string>, } | { "kind": "deal", item: string, discount: number | null, price: number | null, } | { "kind": "event", title: string, rewards: Array<string>, } | { "kind": "alert", rewards: Array<AlertReward>, credits: number | null, } | { "kind": "booster", boost: Boost, factor: number | null, } | { "kind": "nightwave", intermission: boolean, } | { "kind": "official", title: string, note: string | null, type: string, } | { "kind": "given", title: string, detail: string, };

export type DispositionRequest = {
/**
 * `rivens.changes` as published.
 */
changes: unknown,
/**
 * Defaults to 12.
 */
limit?: number, };

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

export type DriftShare = { slug: string, name: string, category: string, priorShare: number, currentShare: number, deltaPp: number, };

export type FeedCoverage = "complete" | "partial" | "unknown";

export type FeedItem = {
/**
 * Stable upstream id where one exists.
 */
id?: string, kind: FeedKind,
/**
 * When it starts, as the snapshot gives it.
 */
at: string,
/**
 * When it ends, where there is an end.
 */
until?: string, text: FeedText,
/**
 * Held slugs this row affects.
 */
affects: Array<string>,
/**
 * False when the row's contents cannot be resolved to slugs, so an empty
 * `affects` must not read as "this doesn't touch you".
 */
affectsKnown: boolean, reach?: FeedReach, stale?: boolean,
/**
 * Whole days since the row's surface was observed; absent when unknown.
 */
dataAgeDays?: number, };

export type FeedKind = "baro" | "vault" | "deal" | "event";

export type FeedReach = "scan" | "none" | "hits" | "partial-hits" | "unknown";

export type FeedText = { "kind": "baro", where: string | null, stock: number, } | { "kind": "vault", packs: number, } | { "kind": "deal", item: string, discount: number | null, price: number | null, } | { "kind": "event", title: string, rewards: number, credits: number, coverage: FeedCoverage, };

export type HandoffCandidate = {
/**
 * Absent when the pick has none, which the page reads as `undefined`.
 */
owned?: unknown, score?: unknown,
/**
 * The entry's `avg` as `Number(avg)`.
 */
avg: number | null, };

export type HandoffPick = { at: number, owned: number, score: number,
/**
 * `owned` x `avg`, rounded as `Math.round` does; `null` when `avg` is `NaN`.
 */
potential: number | null, };

export type HandoffRequest = {
/**
 * The published picks that name a present market entry, in order.
 */
picks: Array<HandoffCandidate>, };

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

export type MetaDrift = { priorYear: number, currentYear: number,
/**
 * Years between the two snapshots that DE did not publish.
 */
missingYears: Array<number>, gains: Array<DriftShare>, losses: Array<DriftShare>, onlyPrior: Array<OnlyShare>, onlyCurrent: Array<OnlyShare>, categoryChanges: number, categories: Array<string>, };

export type MetaDriftRequest = {
/**
 * `usage_history` as published.
 */
usage_history: unknown, };

export type MoverItem = {
/**
 * As a number.
 */
avg: number | null,
/**
 * As `Number(vol)`.
 */
vol: number | null, median_now: number | null, median_90d: number | null, };

export type Movers = { risers: Array<number>, fallers: Array<number>, };

export type MoversRequest = {
/**
 * The present `items` entries, in snapshot order.
 */
items: Array<MoverItem>,
/**
 * Defaults to 20 trades / 48 h.
 */
min_vol?: number,
/**
 * Defaults to 10p.
 */
min_price?: number,
/**
 * Defaults to 8 a side.
 */
limit?: number, };

export type OnlyShare = { slug: string, name: string, category: string, year: number, share: number, };

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

export type SearchRequest = {
/**
 * The query as the page's `trim().toLowerCase()` left it.
 */
query: string, limit: number,
/**
 * The index names that have a market entry: lowercased name, and `vol`
 * as `Number(vol)`.
 */
names: Array<[string, number | null]>, };

export type VaultedRequest = {
/**
 * `vault_status` in snapshot order.
 */
rows: Array<VaultedRow>,
/**
 * Defaults to 12.
 */
limit?: number, };

export type VaultedRow = {
/**
 * The `vault_status` value when it is a string.
 */
status: string | null,
/**
 * The entry's `avg` as a number; `null` without an entry.
 */
avg: number | null, };

export type WebRequest = { "op": "select_picks", "input": PicksRequest } | { "op": "browse_index", "input": BrowseIndexRequest } | { "op": "slug_name", "input": string } | { "op": "search_items", "input": SearchRequest } | { "op": "top_movers", "input": MoversRequest } | { "op": "vaulted_top", "input": VaultedRequest } | { "op": "disposition_changes", "input": DispositionRequest } | { "op": "handoff_sample", "input": HandoffRequest } | { "op": "find_bargains", "input": BargainsRequest } | { "op": "meta_drift", "input": MetaDriftRequest } | { "op": "year_views", "input": YearViewsRequest } | { "op": "price_from_rule", "input": PriceRuleRequest } | { "op": "reasonable_lowest_ask", "input": LiveAskRequest } | { "op": "assess_listings", "input": HealthRequest } | { "op": "select_drifted", "input": DriftRequest } | { "op": "interrupted_batch", "input": BatchRequest } | { "op": "compare_buyers", "input": BuyerCompareRequest } | { "op": "calendar_feed", "input": CalendarRequest } | { "op": "calendar_events", "input": CalendarRequest };

export type WebResponse = { "op": "select_picks", "result": Array<number> } | { "op": "browse_index", "result": BrowseNames } | { "op": "slug_name", "result": string } | { "op": "search_items", "result": Array<number> } | { "op": "top_movers", "result": Movers } | { "op": "vaulted_top", "result": Array<number> } | { "op": "disposition_changes", "result": Array<number> } | { "op": "handoff_sample", "result": Array<HandoffPick> } | { "op": "find_bargains", "result": Array<BargainLead> } | { "op": "meta_drift", "result": MetaDrift | null } | { "op": "year_views", "result": Array<YearView> } | { "op": "price_from_rule", "result": number | null } | { "op": "reasonable_lowest_ask", "result": LiveAskRead } | { "op": "assess_listings", "result": HealthReport } | { "op": "select_drifted", "result": Array<DriftRow> } | { "op": "interrupted_batch", "result": InterruptedBatch | null } | { "op": "compare_buyers", "result": BuyerComparison } | { "op": "calendar_feed", "result": Array<FeedItem> } | { "op": "calendar_events", "result": Array<DatedEvent> };

export type YearStats = { latest: number, latestIdx: number, baseline: number, deltaPct: number | null, high: number, low: number, tradedDays: number, };

export type YearView = { stats: YearStats | null, weekly: Array<number>, };

export type YearViewsRequest = {
/**
 * Each series' daily medians, a non-number as `null`.
 */
medians: Array<Array<number | null>>,
/**
 * Defaults to 20 traded days.
 */
min_days?: number,
/**
 * Defaults to 52.
 */
buckets?: number, };
