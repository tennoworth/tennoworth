// Build it or buy it.
//
// A prime set can be acquired three ways and the app only ever costed one of
// them. DE's recipe tree adds the other two: buy the parts you are missing and
// build (credits + real time), or buy the parts and pay plat to skip the wait.
//
// The honest part is the resources. A recipe's ingredient list includes Orokin
// Cells and Argon Crystals, which the inventory scan cannot see and the market
// cannot price. So a build path carries them as an UNVERIFIED checklist and
// this module never claims a build is cheaper without saying what it did not
// check. Quietly ignoring them would turn "cheaper" into a wrong answer for
// anyone whose foundry is dry.

import { clearingPrice, hasRealPrice } from './sell-priority';
import type { Market, MarketItemEntry, OwnedRecord } from './types';

/** One ingredient line from a recipe. No `slug` means it cannot be bought. */
export interface RecipeIngredient {
  name: string;
  count: number;
  slug?: string;
}

export interface RecipeEntry {
  build_price?: number;
  build_time?: number;
  rush_price?: number;
  ingredients?: RecipeIngredient[];
}

export type BuildPathKind = 'buy-set' | 'buy-parts-build' | 'buy-parts-rush' | 'sell-spares';

export interface BuildPath {
  kind: BuildPathKind;
  /** Plat out of pocket. Negative on the sell-spares path — that one earns. */
  plat: number;
  /** Credits out of pocket, 0 where the path needs none. */
  credits: number;
  /** Seconds of foundry time. */
  seconds: number;
  /** Resources the plan needs but could not verify you have. */
  unverified: RecipeIngredient[];
  /** Plat saved against buying the set outright. Negative means it costs more. */
  savingVsSet: number;
  /** Whether `plat` is a real total. False when the path depends on a part the
   *  snapshot cannot price — the caller must show "—", not the partial sum.
   *  A warning next to an asserted number does not undo the number. */
  platKnown: boolean;
  /** Whether the recipe tree covered this path. False means credits and time
   *  are unknown, not zero and instant. */
  recipesKnown: boolean;
}

function priceOf(market: Market | null | undefined, slug: string | undefined): number | null {
  if (!slug) return null;
  const entry: MarketItemEntry | undefined = market?.items?.[slug];
  // `hasRealPrice`, not `clearingPrice(...) > 0`: clearingPrice floors at 1p,
  // so an item nobody has listed would otherwise cost this plan 1 plat and
  // never register as unpriceable.
  if (!entry || !hasRealPrice(entry)) return null;
  return clearingPrice(entry);
}

export interface SetPart {
  slug: string;
  component_name: string;
}

/**
 * How many of a slug the user holds, across every subtype.
 *
 * The owned map is keyed `${slug}|${subtype ?? ''}`, NOT by bare slug — relics
 * exist as four refinements under one slug and each needs its own row. A bare
 * `owned.get(slug)` therefore silently misses everything, which for this module
 * meant telling people to buy parts they already had.
 */
export function ownedCount(
  owned: Map<string, OwnedRecord> | null | undefined,
  slug: string,
): number {
  if (!owned) return 0;
  let total = 0;
  for (const rec of owned.values()) {
    if (rec.slug === slug) total += rec.count;
  }
  return total;
}

export interface BuildPlan {
  setSlug: string;
  setName: string;
  /** Parts you already hold, with how many. */
  have: Array<{ slug: string; name: string; count: number }>;
  /** Parts you are missing, with what each costs. `price: null` means the
   *  snapshot cannot price it, which makes the whole build path unpriceable. */
  missing: Array<{ slug: string; name: string; price: number | null }>;
  /** What the assembled set sells for. */
  setPrice: number | null;
  paths: BuildPath[];
  /** True when a missing part has no price — every path involving it is a
   *  guess, so the caller must not present a recommendation. */
  incomplete: boolean;
}

/**
 * Cost each way of ending up with one assembled set.
 *
 * `owned` may be absent (the web app has no inventory), in which case every
 * part counts as missing and the comparison is still useful as a general
 * "is this set cheaper as parts" question.
 */
export function planBuild(
  setSlug: string,
  setName: string,
  parts: SetPart[],
  market: Market | null | undefined,
  owned: Map<string, OwnedRecord> | null | undefined,
  recipes: Record<string, RecipeEntry> | null | undefined,
): BuildPlan {
  const have: BuildPlan['have'] = [];
  const missing: BuildPlan['missing'] = [];

  for (const part of parts) {
    const held = ownedCount(owned, part.slug);
    if (held > 0) {
      have.push({ slug: part.slug, name: part.component_name, count: held });
    } else {
      missing.push({
        slug: part.slug,
        name: part.component_name,
        price: priceOf(market, part.slug),
      });
    }
  }

  const setPrice = priceOf(market, setSlug);
  const incomplete = missing.some((m) => m.price == null);
  const partsCost = missing.reduce((sum, m) => sum + (m.price ?? 0), 0);

  // Sum over the PARTS only. The final-assembly recipe is keyed under the
  // blueprint — which is itself a part — because a built frame is not a
  // tradeable item and has no slug of its own. Adding `setSlug` here as well
  // would double-count that step the moment anything is keyed under it.
  let credits = 0;
  let seconds = 0;
  let rush = 0;
  let covered = 0;
  const unverified: RecipeIngredient[] = [];
  for (const slug of parts.map((p) => p.slug)) {
    const recipe = recipes?.[slug];
    if (!recipe) continue;
    covered += 1;
    credits += recipe.build_price ?? 0;
    seconds += recipe.build_time ?? 0;
    rush += recipe.rush_price ?? 0;
    for (const ing of recipe.ingredients ?? []) {
      // Tradeable ingredients are parts we already accounted for; the rest are
      // resources we cannot see.
      if (!ing.slug) unverified.push(ing);
    }
  }

  const recipesKnown = covered > 0;
  const paths: BuildPath[] = [];
  if (setPrice != null) {
    paths.push({
      kind: 'buy-set',
      plat: setPrice,
      credits: 0,
      seconds: 0,
      unverified: [],
      savingVsSet: 0,
      platKnown: true,
      recipesKnown: true,
    });
  }
  paths.push({
    kind: 'buy-parts-build',
    plat: partsCost,
    credits,
    seconds,
    unverified,
    savingVsSet: setPrice == null ? 0 : setPrice - partsCost,
    platKnown: !incomplete,
    recipesKnown,
  });
  if (rush > 0) {
    paths.push({
      kind: 'buy-parts-rush',
      plat: partsCost + rush,
      credits,
      seconds: 0,
      unverified,
      savingVsSet: setPrice == null ? 0 : setPrice - (partsCost + rush),
      platKnown: !incomplete,
      recipesKnown,
    });
  }

  // The fourth option only exists if you hold spares, and it is the one the
  // other three hide: you may not want the frame at all.
  let spareValue = 0;
  let sparesPriced = true;
  for (const h of have) {
    const spares = Math.max(0, h.count - 1);
    if (spares === 0) continue;
    const p = priceOf(market, h.slug);
    if (p == null) {
      sparesPriced = false;
      continue;
    }
    spareValue += p * spares;
  }
  if (spareValue > 0) {
    paths.push({
      kind: 'sell-spares',
      plat: -spareValue,
      credits: 0,
      seconds: 0,
      unverified: [],
      savingVsSet: 0,
      // An unpriced spare is left out of the total, so the figure is a floor
      // rather than the answer — say so instead of overstating precision.
      platKnown: sparesPriced,
      recipesKnown: true,
    });
  }

  return { setSlug, setName, have, missing, setPrice, paths, incomplete };
}

/**
 * The cheapest acquisition path, or null when it cannot be decided.
 *
 * Returns null rather than a best guess when any missing part is unpriced —
 * "build is 43p cheaper" computed with a part silently valued at 0 is exactly
 * the confidently-wrong output this whole feature is supposed to avoid.
 */
export function cheapestPath(plan: BuildPlan): BuildPath | null {
  if (plan.incomplete) return null;
  const acquiring = plan.paths.filter((p) => p.kind !== 'sell-spares');
  if (!acquiring.length) return null;
  return acquiring.reduce((a, b) => (b.plat < a.plat ? b : a));
}

/** "3d 12h" — foundry time in the units the game shows. */
export function humanBuildTime(seconds: number): string {
  if (seconds <= 0) return 'instant';
  const hours = Math.round(seconds / 3600);
  const days = Math.floor(hours / 24);
  const rem = hours % 24;
  if (days > 0) return rem > 0 ? `${days}d ${rem}h` : `${days}d`;
  return `${hours}h`;
}
