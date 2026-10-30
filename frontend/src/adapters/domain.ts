// The desktop's calls into the native market domain (evaluate_domain): the
// typed request envelope, and the inventory, trade-session, advisor and planner
// operations built on it. Each sends only what the native side reads.
import { resolveInvoke, rethrowInvoke } from './runtime';
import type { BaroStock, Inventory, Market, OwnedRecord } from '../contracts/data';
import type { Catalogs } from '../contracts/catalogs';
import type {
  AdvisorRequest, BuildPath, BuildPlan, DomainRequest, DomainResponse, DucatPlan, OwnedItem, RecipeEntry,
  RelicPlanEntry, ScoringMarketEntry, ScrapCandidate, SessionRequest, SetPart, SetReco,
} from '../contracts/generated/domain';
import type { SessionCandidate, SessionPlan } from '../domain/trade-session';
import marketKeys from '../../../tests/fixtures/planners/market-keys.json';

type Operation = DomainRequest['operation'];
type Input<K extends Operation> = Extract<DomainRequest, { operation: K }>['input'];
type Output<K extends Operation> = Extract<DomainResponse, { operation: K }>['result'];

export async function callDomain<K extends Operation>(operation: K, input: Input<K>): Promise<Output<K>> {
  try {
    const response = await resolveInvoke()<DomainResponse>('evaluate_domain', { request: { operation, input } });
    if (!response || response.operation !== operation) throw new Error('The calculation returned an unexpected response. Try again.');
    return response.result as Output<K>;
  } catch (error) { rethrowInvoke(error); }
}

export async function evaluateTradeSession(request: Omit<SessionRequest, 'candidates'> & { candidates: SessionCandidate[] }): Promise<SessionPlan> {
  const result = await callDomain('trade_session', { ...request, candidates: request.candidates.map(row => ({ ...row, subtype: row.subtype ?? undefined })) });
  const candidates = new Map(request.candidates.map(row => [row.key, row]));
  const rows = result.rows.map(row => {
    const source = candidates.get(row.key);
    if (!source) throw new Error('The trade suggestion contains an unknown item. Recalculate before listing.');
    return { ...row, market: source.market };
  });
  return { ...result, rows };
}

function object(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === 'object' && !Array.isArray(value);
}

export function evaluateAdvisor(request: AdvisorRequest) {
  const slugs = [...new Set(request.slugs)];
  const select = (items: unknown, project: (value: unknown) => unknown = value => value) => object(items)
    ? Object.fromEntries(slugs.filter(slug => Object.hasOwn(items, slug)).map(slug => [slug, project(items[slug])]))
    : items;
  // Advice only consumes these price/calendar fields and daily medians. Sending
  // the whole public archive copies thousands of unrelated item histories.
  const market = object(request.market) ? {
    items: select(request.market.items),
    calendar: request.market.calendar,
    set_to_parts: request.market.set_to_parts,
  } : request.market;
  const history = object(request.history) ? {
    start: request.history.start,
    items: select(request.history.items, series => object(series) ? { median: series.median } : series),
  } : request.history;
  return callDomain('advisor', { ...request, slugs, market, history });
}

// ---- inventory ----

export async function normalizeInventoryNative(data: Inventory, catalogs: Catalogs, market: Market) {
  const result = await callDomain('normalize_inventory', {
    inventory: data,
    catalog: [...catalogs.uniqueToInfo].map(([path, info]) => [path, { name: info.name, category: info.category ?? null }]),
    market: {
      catalog: market.catalog ?? {},
      path_to_info: Object.fromEntries(Object.entries(market.path_to_info ?? {}).map(([path, info]) => [path, { ...info, category: info.category ?? null }])),
    },
  });
  return { owned: new Map(result.owned), unresolved: result.unresolved, flatCount: result.flat_count };
}

export async function scoreInventoryNative(owned: Map<string, OwnedRecord>, market: Market, reserveCopies: number, sparesOnly: boolean, availability?: ReadonlyMap<string, number>) {
  const items: Record<string, ScoringMarketEntry> = {};
  for (const rec of owned.values()) {
    const m = market.items[rec.slug];
    if (!m) continue;
    items[rec.slug] = {
      vol: m.vol ?? null, low_sell: m.low_sell ?? null, avg: m.avg ?? null,
      median_now: m.median_now ?? null, median_90d: m.median_90d ?? null,
      low5_avg: m.low5_avg ?? null, ducats: m.ducats ?? null,
      donch_top_90d: m.donch_top_90d ?? null, donch_bot_90d: m.donch_bot_90d ?? null,
      top_buy: m.top_buy ?? null, medians_7d: Array.isArray(m.medians_7d) ? m.medians_7d : [],
    };
  }
  const rows = await callDomain('score_inventory', {
    owned: [...owned].map(([key, rec]): [string, OwnedItem] => [key, { ...rec, subtype: rec.subtype ?? null, kept_lvl: rec.kept_lvl ?? null, leveled: rec.leveled ?? 0 }]),
    market: { items, usage: market.usage ?? {}, set_to_parts: Object.fromEntries(Object.entries(market.set_to_parts ?? {}).map(([slug, entry]) => [slug, { parts: Array.isArray(entry?.parts) ? entry.parts : [] }])) },
    reserve_copies: reserveCopies,
    spares_only: sparesOnly,
    availability: availability ? Object.fromEntries(availability) : undefined,
  });
  return new Map(rows.map(row => [row.key, row]));
}

// ---- planners ----

// Each planner reads only these parts of the snapshot. Sending the whole
// snapshot serialized ~3 MB on the UI thread per call; the native tests run
// every planner on the bundled snapshot with and without the rest.
function plannerMarket(operation: keyof typeof marketKeys, market: Market | null) {
  if (!market) return market;
  return Object.fromEntries(marketKeys[operation].filter(key => key in market).map(key => [key, market[key as keyof Market]]));
}

function records(owned: Map<string, OwnedRecord> | null | undefined) {
  return Array.from(owned?.values() ?? [], ({ slug, name, count, subtype }) => ({ slug, name, count, subtype: subtype ?? null }));
}
export function relicPlan(owned: Map<string, OwnedRecord> | null, market: Market | null, limit = 3): Promise<RelicPlanEntry[]> {
  return callDomain('relic_plan', { owned: records(owned), market: plannerMarket('relic_plan', market), limit: Number.isFinite(limit) ? limit : (owned?.size ?? 0) });
}
export function setRecos(owned: Map<string, OwnedRecord> | null, market: Market | null, limit = 24): Promise<SetReco[]> {
  return callDomain('set_recos', { owned: records(owned), market: plannerMarket('set_recos', market), limit: Number.isFinite(limit) ? limit : Object.keys(market?.set_to_parts ?? {}).length });
}
export async function ducatPlan(owned: Map<string, OwnedRecord> | null, market: Market | null, target: number, keepAbove = 15, availability?: ReadonlyMap<string, number>): Promise<{ candidates: ScrapCandidate[]; plan: DucatPlan }> {
  const availableOwned = availability && owned ? new Map([...owned].map(([key, row]) => [key, { ...row, count: row.subtype || row.slug.endsWith('_set') ? 0 : availability.get(key) ?? 0 }])) : owned;
  const result = await callDomain('ducat_plan', { quantitiesAreAvailable: !!availability, owned: records(availableOwned), market: plannerMarket('ducat_plan', market), target, keepAbove: Number.isFinite(keepAbove) ? keepAbove : Number.MAX_SAFE_INTEGER });
  const hydrate = (candidate: Omit<ScrapCandidate, 'ducatsPerPlat'> & { ducatsPerPlat: number | null }): ScrapCandidate => ({ ...candidate, ducatsPerPlat: candidate.ducatsPerPlat ?? Infinity });
  return { candidates: result.candidates.map(hydrate), plan: { ...result.plan, picks: result.plan.picks.map(hydrate), heldBack: result.plan.heldBack.map(hydrate) } };
}
export function buildPlan(setSlug: string, setName: string, parts: SetPart[], market: Market | null, owned: Map<string, OwnedRecord> | null, recipes: Record<string, RecipeEntry> | null | undefined): Promise<{ plan: BuildPlan; cheapest: BuildPath | null }> {
  return callDomain('build_plan', { setSlug, setName, parts, market: plannerMarket('build_plan', market), owned: records(owned), recipes: recipes ?? null });
}

export function baroValue(stock: BaroStock[], market: Market | null, owned: Map<string, OwnedRecord> | null) {
  return callDomain('baro_value', { stock, market: plannerMarket('baro_value', market), owned: owned ? records(owned) : null });
}
