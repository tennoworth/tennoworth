// Derived from Rust wire contracts. Run the binding export test to update.

export type Advice = "sell_now" | "hold" | "neutral";

export type AlertReward = { item: string, count: number, };

export type Appraisal = { dist: Distribution | null,
/**
 * Set when DE published no usable price for this tier.
 */
unavailable?: Unavailable, placement: Placement | null, verdict: PriceVerdict | null, reroll: RerollRead | null,
/**
 * True when the average is dragged well above the median - the average is
 * then not a typical price and the UI must lead with the median.
 */
skewed: boolean,
/**
 * Notes the UI must show alongside any number - the limits of the model.
 */
caveats: Array<AppraisalCaveat>, };

export type AppraisalCaveat = "no-data" | "band" | "desirability" | "spliced" | "rare" | "skewed";

export type AppraiseRequest = { price: number | null, tier: RivenTier | null,
/**
 * Null reads as a fresh riven.
 */
rerolls: number | null, locked: boolean, };

export type AuctionStatSign = { url_name: string, positive: boolean, };

export type AvailableTag = { tag: string, count: number, group: TagGroup, };

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

export type CompAsk = { owner_status: string | null, created_ms: number | null, };

export type CompsSampleRead = { size: number, dated: number, oldestAskDays: number | null, newestAskDays: number | null, status: CompsStatusCounts, };

export type CompsStatusCounts = { online: number, ingame: number, offline: number, unknown: number, };

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

export type DescribeCompsRequest = { comps: Array<CompAsk>, now_ms: number | null, };

export type DiffOwnedRequest = {
/**
 * The last scan's `[key, count]` pairs; `null` when there is none.
 */
previous: Array<[string, JsNumber]> | null, current: Array<[string, JsNumber]>, };

export type DispositionRequest = {
/**
 * `rivens.changes` as published.
 */
changes: unknown,
/**
 * Defaults to 12.
 */
limit?: number, };

export type Distribution = { median: number, avg: number, min: number, max: number, stddev: number,
/**
 * DE's 0-100 relative popularity - see [`LOW_POPULARITY`].
 */
pop: number, };

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

export type ExtractRivensRequest = {
/**
 * The inventory's `Upgrades`, as DE wrote it.
 */
upgrades: unknown, };

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

export type OtherSellers = { items: number, plat: number, };

export type OwnedRiven = {
/**
 * The full /Lotus/Upgrades/Mods/Randomized/... path.
 */
path: string,
/**
 * Weapon /Lotus/... path from the fingerprint; null on veiled rivens.
 */
compat: string | null,
/**
 * WFM weapon slug, resolved via the weapons manifest's `game_ref`.
 */
slug: string | null,
/**
 * WFM weapon display name once resolved.
 */
weaponName: string | null, rerolls: number, lvl: number, pol: string | null, buffs: Array<RivenFingerprintStat>, curses: Array<RivenFingerprintStat>,
/**
 * A challenge instead of stats - the riven is veiled.
 */
veiled: boolean,
/**
 * DE's fingerprint string exactly as scanned. Kept because DE adds riven
 * state (locks, splices) before anyone knows its keys, and the parsed
 * fields above drop what they do not name. Absent on snapshots saved
 * before it was kept.
 */
raw?: string, };

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

export type Placement = "below-observed" | "bottom" | "middle" | "upper" | "above-observed";

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

export type PriceVerdict = "below" | "fair" | "above" | "outlier";

export type RerollRead = {
/**
 * Kuva this spin costs.
 */
kuva: number,
/**
 * The weapon's median trade.
 */
median: number,
/**
 * Whether the offer is above that median. A fact about two published
 * numbers, not a forecast.
 */
aboveMedian: boolean, };

export type ResolveRivensRequest = { rivens: Array<RivenCompat>,
/**
 * Each manifest weapon's `game_ref`, in manifest order.
 */
game_refs: Array<string | null>, };

export type RivenAttributeRef = { game_ref?: string, slug?: string, name?: string, };

export type RivenCompat = { compat: string | null, veiled: boolean, };

export type RivenFingerprintStat = { tag: string, value: number, };

export type RivenRow = { stats: Array<RivenStatRead>, splices: Array<SpliceChoice>, filter: RivenStatFilterPlan | null, unread: Array<string>, };

export type RivenRowInput = { buffs: Array<RivenStatTag>, curses: Array<RivenStatTag>, raw?: string, veiled: boolean,
/**
 * The weapon's WFM `riven_type`, which picks the splice recipes.
 */
riven_type?: string, };

export type RivenRowsRequest = { attributes: Array<RivenAttributeRef>, rivens: Array<RivenRowInput>, };

export type RivenSimilarityRequest = { attributes: Array<RivenAttributeRef>, buffs: Array<RivenStatTag>, curses: Array<RivenStatTag>,
/**
 * Each auction's stats.
 */
auctions: Array<Array<AuctionStatSign>>, };

export type RivenStatFilter = { positive: Array<string>, negative: Array<string>, };

export type RivenStatFilterPlan = { filter: RivenStatFilter,
/**
 * False when a positive stat could not be resolved to a WFM slug, so the
 * filter is broader than the riven.
 */
complete: boolean, };

export type RivenStatRead = { tag: string, slug: string | null, positive: boolean, name: string | null, spliced: boolean, };

export type RivenStatTag = { tag: string, };

export type RivenTier = { avg: number, median: number, min: number, max: number, stddev: number, pop: number, };

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

export type SpliceChoice = { recipe: SpliceRecipe,
/**
 * The ingredients as they sit on the riven, in recipe order: indexes into
 * the row's stats.
 */
consumed: [number, number],
/**
 * DE: "if one of the consumed traits was a negative bonus, the random
 * trait will also be a negative".
 */
randomIsNegative: boolean, };

export type SpliceRecipe = { result: string,
/**
 * DE's name for the spliced trait, for when the snapshot's manifest
 * predates it.
 */
name: string, ingredients: [string, string], scope: SpliceScope, };

export type SpliceScope = "ranged" | "melee" | "all";

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

export type Unavailable = "no-data";

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

export type VisitPhase = "here" | "incoming" | "unknown";

export type WebRequest = { "op": "select_picks", "input": PicksRequest } | { "op": "browse_index", "input": BrowseIndexRequest } | { "op": "slug_name", "input": string } | { "op": "search_items", "input": SearchRequest } | { "op": "top_movers", "input": MoversRequest } | { "op": "vaulted_top", "input": VaultedRequest } | { "op": "disposition_changes", "input": DispositionRequest } | { "op": "handoff_sample", "input": HandoffRequest } | { "op": "find_bargains", "input": BargainsRequest } | { "op": "meta_drift", "input": MetaDriftRequest } | { "op": "year_views", "input": YearViewsRequest } | { "op": "price_from_rule", "input": PriceRuleRequest } | { "op": "reasonable_lowest_ask", "input": LiveAskRequest } | { "op": "assess_listings", "input": HealthRequest } | { "op": "select_drifted", "input": DriftRequest } | { "op": "interrupted_batch", "input": BatchRequest } | { "op": "compare_buyers", "input": BuyerCompareRequest } | { "op": "calendar_feed", "input": CalendarRequest } | { "op": "calendar_events", "input": CalendarRequest } | { "op": "sell_results", "input": SellFilterRequest } | { "op": "sell_tags", "input": SellFilterRequest } | { "op": "sell_empty_reason", "input": SellFilterRequest } | { "op": "tag_groups", "input": Array<string> } | { "op": "top_tags", "input": TopTagsRequest } | { "op": "split_by_trades", "input": TradeSplitRequest } | { "op": "ledger_totals", "input": Array<LedgerTrade> } | { "op": "ledger_since", "input": LedgerSinceRequest } | { "op": "sold_by_item", "input": Array<LedgerTrade> } | { "op": "top_sellers", "input": TopSellersRequest } | { "op": "median_week", "input": Array<WeekPlat> } | { "op": "ducat_basket", "input": BasketRequest } | { "op": "baro_phase", "input": PhaseRequest } | { "op": "diff_owned", "input": DiffOwnedRequest } | { "op": "extract_rivens", "input": ExtractRivensRequest } | { "op": "resolve_rivens", "input": ResolveRivensRequest } | { "op": "riven_rows", "input": RivenRowsRequest } | { "op": "riven_similarity", "input": RivenSimilarityRequest } | { "op": "spliced_traits", "input": null } | { "op": "appraise_riven", "input": AppraiseRequest } | { "op": "describe_riven_comps", "input": DescribeCompsRequest };

export type WebResponse = { "op": "select_picks", "result": Array<number> } | { "op": "browse_index", "result": BrowseNames } | { "op": "slug_name", "result": string } | { "op": "search_items", "result": Array<number> } | { "op": "top_movers", "result": Movers } | { "op": "vaulted_top", "result": Array<number> } | { "op": "disposition_changes", "result": Array<number> } | { "op": "handoff_sample", "result": Array<HandoffPick> } | { "op": "find_bargains", "result": Array<BargainLead> } | { "op": "meta_drift", "result": MetaDrift | null } | { "op": "year_views", "result": Array<YearView> } | { "op": "price_from_rule", "result": number | null } | { "op": "reasonable_lowest_ask", "result": LiveAskRead } | { "op": "assess_listings", "result": HealthReport } | { "op": "select_drifted", "result": Array<DriftRow> } | { "op": "interrupted_batch", "result": InterruptedBatch | null } | { "op": "compare_buyers", "result": BuyerComparison } | { "op": "calendar_feed", "result": Array<FeedItem> } | { "op": "calendar_events", "result": Array<DatedEvent> } | { "op": "sell_results", "result": Array<SellResult> } | { "op": "sell_tags", "result": Array<AvailableTag> } | { "op": "sell_empty_reason", "result": EmptyReason } | { "op": "tag_groups", "result": Array<TagGroup> } | { "op": "top_tags", "result": Array<string> } | { "op": "split_by_trades", "result": TradeSplit } | { "op": "ledger_totals", "result": LedgerTotals } | { "op": "ledger_since", "result": Array<number> } | { "op": "sold_by_item", "result": Array<SoldItemTotal> } | { "op": "top_sellers", "result": TopSellers } | { "op": "median_week", "result": number | null } | { "op": "ducat_basket", "result": DucatBasket } | { "op": "baro_phase", "result": BaroPhase } | { "op": "diff_owned", "result": Array<[string, number]> } | { "op": "extract_rivens", "result": Array<OwnedRiven> } | { "op": "resolve_rivens", "result": Array<number | null> } | { "op": "riven_rows", "result": Array<RivenRow> } | { "op": "riven_similarity", "result": Array<number | null> } | { "op": "spliced_traits", "result": Array<string> } | { "op": "appraise_riven", "result": Appraisal } | { "op": "describe_riven_comps", "result": CompsSampleRead };

export type WeekPlat = { plat: number,
/**
 * The current week, still running.
 */
partial: boolean, };

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
