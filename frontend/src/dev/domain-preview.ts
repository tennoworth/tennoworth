import type { DomainRequest, PlannerOwned, ScoredInventoryFact, SessionRow } from '../contracts/generated/domain';
import type { Inventory, Market, OwnedRecord, SlimItemInfo } from '../contracts/data';
import { normalizeInventoryPreview } from '../domain/normalize-inventory';
import { computeResults, type FilterState } from '../domain/filter-engine';
import { selectSession, type SessionCandidate } from '../domain/trade-session';
import { adviseOwned, slope30 } from '../domain/advisor';
import { points, weekly, yearStats, type History } from '../domain/history';
import { deriveRelicPlan } from '../domain/relic-planner';
import { deriveSetRecos } from '../domain/set-recos';
import { scrapCandidates, planDucats } from '../domain/ducat-plan';
import { planBuild, cheapestPath } from '../domain/build-cost';

const sessionFields = new Set<string>([
  'key', 'slug', 'name', 'owned', 'sellable', 'leveled', 'subtype', 'type', 'hold', 'bulk', 'supported', 'market',
  'quantity', 'per_trade', 'platinum', 'trades', 'reason', 'bid',
] satisfies Array<keyof SessionRow>);
const filters: FilterState = {
  minPrice: -Infinity, minOwned: 0, typeFilter: 'all', hideAtLvl: Infinity,
  activeTags: new Set(), vaultOnly: false, ducatsOnly: false, minVol: 0,
  minMedian: 0, typesAny: [], sparesOnly: false, adviceOnly: false,
};
function plannerOwned(rows: PlannerOwned[]): Map<string, OwnedRecord> {
  // Distinct input rows may share a slug; the planners aggregate their counts.
  return new Map(rows.map((row, index) => [String(index), { ...row, type: '', kept_lvl: null, leveled: 0 }]));
}
function scorePreview(input: Extract<DomainRequest, { operation: 'score_inventory' }>['input']): ScoredInventoryFact[] {
  const out: ScoredInventoryFact[] = [];
  const market = { ...input.market, items: Object.fromEntries(Object.entries(input.market.items)
    .map(([slug, row]) => [slug, { ...row, avg: row.avg ?? 0, vol: row.vol ?? 0 }])) };
  for (const [key, row] of input.owned) {
    // Use the existing reserve calculation to express spares without applying
    // the view's spares filter: the native response includes zero-sellable rows.
    const reserve = input.spares_only ? row.leveled + ((row.kept_lvl ?? 0) > 0 ? 0 : 1) : input.reserve_copies;
    const result = computeResults(new Map([[key, row]]), market as unknown as Market, filters, reserve)[0];
    if (!result) continue;
    const { sellable, clearing_price, sell_score, patience, ducats, plat_per_100d, potential_plat, raw_value,
      medians_7d, median_90d, delta_90d_pct, timing, demand } = result;
    out.push({ key, sellable, clearing_price, sell_score, patience, ducats, plat_per_100d, potential_plat,
      raw_value, medians_7d, median_90d, delta_90d_pct, timing, demand });
  }
  return out;
}

/** Only the development preview installs this implementation as its IPC host. */
export function evaluateDomainPreview(request: DomainRequest): unknown {
  let result: unknown;
  switch (request.operation) {
    case 'normalize_inventory': {
      const input = request.input;
      const normalized = normalizeInventoryPreview(input.inventory as Inventory,
        { uniqueToInfo: new Map(input.catalog as Array<[string, SlimItemInfo]>) }, input.market as unknown as Market);
      result = { owned: [...normalized.owned], unresolved: normalized.unresolved, flat_count: normalized.flatCount };
      break;
    }
    case 'score_inventory': result = scorePreview(request.input); break;
    case 'trade_session': {
      const { candidates, mode, budget, target } = request.input;
      const plan = selectSession(candidates as SessionCandidate[], mode, budget, target ?? undefined);
      result = { ...plan, rows: plan.rows.map(row => Object.fromEntries(Object.entries(row)
        .filter(([key, value]) => sessionFields.has(key) && !((key === 'subtype' || key === 'supported') && value == null)))) };
      break;
    }
    case 'advisor': {
      const { slugs, market, history, now_ms } = request.input;
      result = Object.fromEntries(adviseOwned(slugs, market as Market, history as History | null, now_ms));
      break;
    }
    case 'history': {
      const { series, min_days, buckets } = request.input;
      result = { points: points(series), stats: yearStats(series, min_days ?? 20), weekly: weekly(series, buckets ?? 52), slope30: slope30(series.median) };
      break;
    }
    case 'relic_plan': {
      const { owned, market, limit } = request.input;
      result = deriveRelicPlan(plannerOwned(owned), market as Market, limit ?? 3);
      break;
    }
    case 'set_recos': {
      const { owned, market, limit } = request.input;
      result = deriveSetRecos(plannerOwned(owned), market as Market, limit ?? 24);
      break;
    }
    case 'ducat_plan': {
      const { owned, market, target, keepAbove } = request.input;
      const candidates = scrapCandidates(plannerOwned(owned), market as Market);
      result = { candidates, plan: planDucats(candidates, target, keepAbove ?? 15) };
      break;
    }
    case 'build_plan': {
      const { setSlug, setName, parts, market, owned, recipes } = request.input;
      const plan = planBuild(setSlug, setName, parts, market as Market, plannerOwned(owned), recipes);
      result = { plan, cheapest: cheapestPath(plan) };
      break;
    }
    default: throw new Error('Unknown domain preview operation');
  }
  // The preview runs without an IPC serialization boundary. Reproduce it so
  // unknown ratios (Infinity) and optional fields match the native JSON DTO.
  return JSON.parse(JSON.stringify({ operation: request.operation, result }));
}
