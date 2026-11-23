// Derived from Rust wire contracts. Run the binding export test to update.

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

export type CompAsk = { owner_status: string | null, created_ms: number | null, };

export type CompsSampleRead = { size: number, dated: number, oldestAskDays: number | null, newestAskDays: number | null, status: CompsStatusCounts, };

export type CompsStatusCounts = { online: number, ingame: number, offline: number, unknown: number, };

export type DescribeCompsRequest = { comps: Array<CompAsk>, now_ms: number | null, };

export type Distribution = { median: number, avg: number, min: number, max: number, stddev: number,
/**
 * DE's 0-100 relative popularity - see [`LOW_POPULARITY`].
 */
pop: number, };

export type ExtractRivensRequest = {
/**
 * The inventory's `Upgrades`, as DE wrote it.
 */
upgrades: unknown, };

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

export type Unavailable = "no-data";

export type WebRequest = { "op": "select_picks", "input": PicksRequest } | { "op": "extract_rivens", "input": ExtractRivensRequest } | { "op": "resolve_rivens", "input": ResolveRivensRequest } | { "op": "riven_rows", "input": RivenRowsRequest } | { "op": "riven_similarity", "input": RivenSimilarityRequest } | { "op": "spliced_traits", "input": null } | { "op": "appraise_riven", "input": AppraiseRequest } | { "op": "describe_riven_comps", "input": DescribeCompsRequest };

export type WebResponse = { "op": "select_picks", "result": Array<number> } | { "op": "extract_rivens", "result": Array<OwnedRiven> } | { "op": "resolve_rivens", "result": Array<number | null> } | { "op": "riven_rows", "result": Array<RivenRow> } | { "op": "riven_similarity", "result": Array<number | null> } | { "op": "spliced_traits", "result": Array<string> } | { "op": "appraise_riven", "result": Appraisal } | { "op": "describe_riven_comps", "result": CompsSampleRead };
