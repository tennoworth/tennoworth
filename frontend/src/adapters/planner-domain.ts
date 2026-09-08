import { callDomain } from './domain-commands';
import type { Market, OwnedRecord } from '../contracts/data';
import type { BuildPath, BuildPlan, RecipeEntry, SetPart } from '../domain/build-cost';
import type { DucatPlan, ScrapCandidate } from '../domain/ducat-plan';
import type { RelicPlanEntry } from '../domain/relic-planner';
import type { SetReco } from '../domain/set-recos';

function records(owned: Map<string, OwnedRecord> | null | undefined) {
  return Array.from(owned?.values() ?? [], ({ slug, name, count, subtype }) => ({ slug, name, count, subtype: subtype ?? null }));
}
export function relicPlan(owned: Map<string, OwnedRecord> | null, market: Market | null, limit = 3): Promise<RelicPlanEntry[]> {
  return callDomain('relic_plan', { owned: records(owned), market, limit: Number.isFinite(limit) ? limit : (owned?.size ?? 0) });
}
export function setRecos(owned: Map<string, OwnedRecord> | null, market: Market | null, limit = 24): Promise<SetReco[]> {
  return callDomain('set_recos', { owned: records(owned), market, limit: Number.isFinite(limit) ? limit : Object.keys(market?.set_to_parts ?? {}).length });
}
export async function ducatPlan(owned: Map<string, OwnedRecord> | null, market: Market | null, target: number, keepAbove = 15, availability?: ReadonlyMap<string, number>): Promise<{ candidates: ScrapCandidate[]; plan: DucatPlan }> {
  const availableOwned = availability && owned ? new Map([...owned].map(([key, row]) => [key, { ...row, count: row.subtype || row.slug.endsWith('_set') ? 0 : availability.get(key) ?? 0 }])) : owned;
  const result = await callDomain('ducat_plan', { quantitiesAreAvailable: !!availability, owned: records(availableOwned), market, target, keepAbove: Number.isFinite(keepAbove) ? keepAbove : Number.MAX_SAFE_INTEGER });
  const hydrate = (candidate: Omit<ScrapCandidate, 'ducatsPerPlat'> & { ducatsPerPlat: number | null }): ScrapCandidate => ({ ...candidate, ducatsPerPlat: candidate.ducatsPerPlat ?? Infinity });
  return { candidates: result.candidates.map(hydrate), plan: { ...result.plan, picks: result.plan.picks.map(hydrate), heldBack: result.plan.heldBack.map(hydrate) } };
}
export function buildPlan(setSlug: string, setName: string, parts: SetPart[], market: Market | null, owned: Map<string, OwnedRecord> | null, recipes: Record<string, RecipeEntry> | null | undefined): Promise<{ plan: BuildPlan; cheapest: BuildPath | null }> {
  return callDomain('build_plan', { setSlug, setName, parts, market, owned: records(owned), recipes: recipes ?? null });
}
