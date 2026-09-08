// Derived from Rust wire contracts. Run the binding export test to update.

export type Advice = "sell_now" | "hold" | "neutral";

export type AdvisorRequest = { slugs: Array<string>, market: unknown, history: unknown, now_ms: number, };

export type BuildPath = { kind: BuildPathKind, plat: number, credits: number, seconds: number, unverified: Array<RecipeIngredient>, savingVsSet: number, platKnown: boolean, recipesKnown: boolean, };

export type BuildPathKind = "buy-set" | "buy-parts-build" | "buy-parts-rush" | "sell-spares";

export type BuildPlan = { setSlug: string, setName: string, have: Array<HavePart>, missing: Array<MissingPart>, setPrice: number | null, paths: Array<BuildPath>, incomplete: boolean, };

export type BuildRequest = { setSlug: string, setName: string, parts: Array<SetPart>, market: unknown, owned: Array<PlannerOwned>, recipes: { [key in string]: RecipeEntry } | null, };

export type BuildResult = { plan: BuildPlan, cheapest: BuildPath | null, };

export type CatalogItem = { name: string, category: string | null, };

export type DemandRead = { usage: UsageEntry | null, inherited: boolean, liquidity: Liquidity, band: MasteryBand | null, };

export type DomainRequest = { "operation": "normalize_inventory", "input": InventoryRequest } | { "operation": "score_inventory", "input": ScoreInventoryRequest } | { "operation": "trade_session", "input": SessionRequest } | { "operation": "advisor", "input": AdvisorRequest } | { "operation": "history", "input": HistoryRequest } | { "operation": "relic_plan", "input": PlannerRequest } | { "operation": "set_recos", "input": PlannerRequest } | { "operation": "ducat_plan", "input": DucatRequest } | { "operation": "build_plan", "input": BuildRequest };

export type DomainResponse = { "operation": "normalize_inventory", "result": NormalizedInventory } | { "operation": "score_inventory", "result": Array<ScoredInventoryFact> } | { "operation": "trade_session", "result": SessionPlan } | { "operation": "advisor", "result": { [key in string]: Verdict } } | { "operation": "history", "result": HistoryAnalysis } | { "operation": "relic_plan", "result": Array<RelicPlanEntry> } | { "operation": "set_recos", "result": Array<SetReco> } | { "operation": "ducat_plan", "result": DucatResult } | { "operation": "build_plan", "result": BuildResult };

export type DucatPlan = { picks: Array<ScrapCandidate>, ducats: number, platGivenUp: number, short: number, heldBack: Array<ScrapCandidate>, };

export type DucatRequest = { owned: Array<PlannerOwned>, market: unknown, target: number, keepAbove: number | null, quantitiesAreAvailable?: boolean, };

export type DucatResult = { candidates: Array<ScrapCandidate>, plan: DucatPlan, };

export type HavePart = { slug: string, name: string, count: number, };

export type HistoryAnalysis = { points: Array<[number, number]>, stats: YearStats | null, weekly: Array<number>, slope30: number | null, };

export type HistoryRequest = { series: HistorySeries, min_days: number | null, buckets: number | null, };

export type HistorySeries = { median: Array<number | null>, volume: Array<number>, subtype: string | null, };

export type InventoryMarket = { catalog: { [key in string]: string }, path_to_info: { [key in string]: InventoryPathInfo }, };

export type InventoryPathInfo = { name: string, slug: string, category: string | null, };

export type InventoryRequest = { inventory: unknown, catalog: Array<[string, CatalogItem]>, market: InventoryMarket, };

export type Liquidity = "sells-today" | "underpriced" | "slow" | "thin" | "unknown";

export type MasteryBand = { from: number, to: number, };

export type MissingPart = { slug: string, name: string, price: number | null, };

export type MissingSetPart = { slug: string, name: string, quantity: number, low_sell: number, };

export type NormalizedInventory = { owned: Array<[string, OwnedItem]>, unresolved: Record<string, number>, flat_count: number, };

export type OwnedItem = { count: number, name: string, type: string, slug: string, subtype: string | null, kept_lvl: number | null, leveled: number, };

export type PlannerOwned = { slug: string, name: string, count: number, subtype: string | null, };

export type PlannerRequest = { owned: Array<PlannerOwned>, market: unknown, limit: number | null, };

export type RecipeEntry = { build_price?: number, build_time?: number, rush_price?: number, ingredients?: Array<RecipeIngredient>, };

export type RecipeIngredient = { name: string, count: number, slug?: string, };

export type Refinement = "intact" | "exceptional" | "flawless" | "radiant";

export type RelicDecision = { ladder: Array<RelicEv>, best: RelicEv | null, sellNow: number, movingCount: number, totalRewards: number, verdict: RelicVerdict, };

export type RelicEv = { refinement: Refinement, ev: number, traces: number, gainOverIntact: number, platPerTrace: number | null, };

export type RelicPlanEntry = { relic_slug: string, relic_name: string, owned: number, epp: number, epp_owned: number, sell_now: number, moving_count: number, total_rewards: number, rewards: Array<RelicPlanReward>, decision?: RelicDecision, };

export type RelicPlanReward = { slug: string, name: string, rarity: string, chance: number, low_sell: number, vol_48h: number, };

export type RelicVerdict = "crack" | "refine" | "sell-intact" | "thin" | "unknown";

export type ScoreInventoryRequest = { owned: Array<[string, OwnedItem]>, market: ScoringMarket, reserve_copies: number, spares_only: boolean, availability?: { [key in string]: number }, };

export type ScoredInventoryFact = { key: string, sellable: number, clearing_price: number, sell_score: number, patience: boolean, ducats: number | null, plat_per_100d: number | null, potential_plat: number, raw_value: number, medians_7d: Array<number>, median_90d: number | null, delta_90d_pct: number | null, timing: Timing, demand: DemandRead, };

export type ScoringMarket = { items: { [key in string]: ScoringMarketEntry }, usage: Record<string, unknown>, set_to_parts: { [key in string]: UsageSet }, };

export type ScoringMarketEntry = { vol: number | null, low_sell: number | null, avg: number | null, median_now: number | null, median_90d: number | null, low5_avg: number | null, ducats: number | null, donch_top_90d: number | null, donch_bot_90d: number | null, top_buy: number | null, medians_7d: Array<number>, };

export type ScrapCandidate = { slug: string, name: string, spare: number, ducats: number, plat: number,
/**
 * Null is an unpriced part: it sorts before finite ratios and crosses JSON without infinity.
 */
ducatsPerPlat: number | null, totalDucats: number, totalPlat: number, };

export type SessionCandidate = { key: string, slug: string, name: string, owned: number, sellable: number, leveled: number, subtype?: string, type: string, hold: boolean, bulk: boolean, supported?: boolean, market: unknown, components?: { [key in string]: number }, };

export type SessionExclusion = { name: string, reason: string, };

export type SessionMode = "fast" | "per-trade" | "clear" | "max";

export type SessionPlan = { rows: Array<SessionRow>, trades: number, total: number, excluded: Array<SessionExclusion>, target: number | null, shortfall: number | null, };

export type SessionRequest = { candidates: Array<SessionCandidate>, mode: SessionMode, budget: number, target: number | null, };

export type SessionRow = { component_limits: { [key in string]: number }, quantity: number, per_trade: number, platinum: number, trades: number, reason: string, bid: number | null, key: string, slug: string, name: string, owned: number, sellable: number, leveled: number, subtype?: string, type: string, hold: boolean, bulk: boolean, supported?: boolean, market: unknown, components?: { [key in string]: number }, };

export type SetPart = { slug: string, component_name: string, };

export type SetReco = { kind: SetRecoKind, set_slug: string, set_name: string, set_low_sell?: number, set_top_buy?: number, set_vol?: number, parts: Array<SetRecoPart>, parts_low_sell?: number, missing?: Array<MissingSetPart>, missing_cost?: number, instant_uplift?: number, extras?: number, extras_plat?: number, net_plat: number, };

export type SetRecoKind = "near-complete" | "complete-with-extras" | "extras";

export type SetRecoPart = { slug: string, name: string, count: number, required: number, low_sell: number, };

export type Timing = "hold" | "peak" | "neutral";

export type UsageEntry = { name: string, category: string, year: number, share: number, peak_mr: number, by_mr: Array<number>, };

export type UsageSet = { parts: Array<UsageSetPart>, };

export type UsageSetPart = { slug: string, };

export type Verdict = { advice: Advice, reasons: Array<string>, };

export type YearStats = { latest: number, latestIdx: number, baseline: number, deltaPct: number | null, high: number, low: number, tradedDays: number, };
