// Derived from Rust wire contracts. Run the binding export test to update.

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

export type DispositionRequest = {
/**
 * `rivens.changes` as published.
 */
changes: unknown,
/**
 * Defaults to 12.
 */
limit?: number, };

export type DriftShare = { slug: string, name: string, category: string, priorShare: number, currentShare: number, deltaPp: number, };

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

export type WebRequest = { "op": "select_picks", "input": PicksRequest } | { "op": "browse_index", "input": BrowseIndexRequest } | { "op": "slug_name", "input": string } | { "op": "search_items", "input": SearchRequest } | { "op": "top_movers", "input": MoversRequest } | { "op": "vaulted_top", "input": VaultedRequest } | { "op": "disposition_changes", "input": DispositionRequest } | { "op": "handoff_sample", "input": HandoffRequest } | { "op": "find_bargains", "input": BargainsRequest } | { "op": "meta_drift", "input": MetaDriftRequest } | { "op": "year_views", "input": YearViewsRequest };

export type WebResponse = { "op": "select_picks", "result": Array<number> } | { "op": "browse_index", "result": BrowseNames } | { "op": "slug_name", "result": string } | { "op": "search_items", "result": Array<number> } | { "op": "top_movers", "result": Movers } | { "op": "vaulted_top", "result": Array<number> } | { "op": "disposition_changes", "result": Array<number> } | { "op": "handoff_sample", "result": Array<HandoffPick> } | { "op": "find_bargains", "result": Array<BargainLead> } | { "op": "meta_drift", "result": MetaDrift | null } | { "op": "year_views", "result": Array<YearView> };

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
