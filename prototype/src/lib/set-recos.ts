// Derives "set completion" recommendations from the user's owned items
// against the market.set_to_parts map (which the scraper bakes from the
// warframestat parent walk). Three reco kinds, in display priority:
//
//   1. `near-complete` — own ≥ (N-1) of N parts, missing 1 or 2.
//      Action: buy the missing part(s), sell as a full set.
//      Net = set_low_sell − sum(low_sell of missing parts).
//      Worth surfacing when Net > sum(low_sell of parts you already own)
//      — i.e. flipping is genuinely better than selling-as-parts.
//
//   2. `complete-with-extras` — own every part AND extras of one or more.
//      Each "extra copy" of a part is sellable as that part.
//      Headline: how much plat the extras add up to at low_sell.
//
//   3. `extras` — own multiple copies of some parts of a NON-complete set.
//      Just count the duplicates and their plat — no set-flip story.
//
// Returns an array sorted by `net_plat` desc, capped at `limit`. Pure
// function — easy to unit test, no Svelte state.

import type { Market, OwnedRecord } from './types';

type SetRecoKind = 'near-complete' | 'complete-with-extras' | 'extras';

interface SetRecoPart {
  slug: string;
  name: string;
  count: number;
  low_sell: number;
}

export interface SetReco {
  kind: SetRecoKind;
  set_slug: string;
  set_name: string;
  set_low_sell?: number;
  set_vol?: number;           // 48h closed-trade volume of the assembled set
  parts: SetRecoPart[];
  parts_low_sell?: number;
  missing?: Array<{ slug: string; name: string; low_sell: number }>;
  missing_cost?: number;
  extras?: number;
  extras_plat?: number;
  net_plat: number;
}

export function deriveSetRecos(
  owned: Map<string, OwnedRecord> | null | undefined,
  market: Market | null | undefined,
  limit = 24,
): SetReco[] {
  const sets = market?.set_to_parts;
  if (!sets || !owned) return [];

  // Build a slug -> count map from owned. Skip relic refinements
  // (subtyped keys) — they aren't part of a prime SET.
  const ownedBySlug = new Map<string, number>();
  for (const rec of owned.values()) {
    if (rec.subtype) continue;
    ownedBySlug.set(rec.slug, (ownedBySlug.get(rec.slug) || 0) + rec.count);
  }

  const out: SetReco[] = [];
  for (const [setSlug, info] of Object.entries(sets)) {
    const parts = info.parts;
    if (!Array.isArray(parts) || parts.length === 0) continue;
    const setEntry = market.items?.[setSlug];

    // For each part: count owned + low_sell price.
    let ownedDistinct = 0;
    let totalOwned = 0;
    let extraCopies = 0;
    let extrasPlat = 0;
    let partsLowSellSum = 0;     // value if you sold the parts individually
    const missing: Array<{ slug: string; name: string; low_sell: number }> = [];
    const partRows: SetRecoPart[] = [];
    for (const p of parts) {
      const cnt = ownedBySlug.get(p.slug) || 0;
      const me = market.items?.[p.slug];
      const lowSell = me?.low_sell || 0;
      partsLowSellSum += lowSell;
      partRows.push({ slug: p.slug, name: p.component_name, count: cnt, low_sell: lowSell });
      if (cnt > 0) {
        ownedDistinct += 1;
        totalOwned += cnt;
        if (cnt > 1) {
          extraCopies += cnt - 1;
          extrasPlat += (cnt - 1) * lowSell;
        }
      } else {
        missing.push({ slug: p.slug, name: p.component_name, low_sell: lowSell });
      }
    }
    if (ownedDistinct === 0) continue;

    const setLowSell = setEntry?.low_sell || 0;
    const setVol = setEntry?.vol || 0;
    const complete = ownedDistinct === parts.length;

    if (complete) {
      // Only surface when there's actually something to list. A complete
      // set with no spares produces net_plat = 0 and a meaningless
      // "plus 0 spare blueprints" caption — dropping it keeps the card
      // honest. The user already has the set; if they wanted to know it
      // exists they'd check the in-game inventory, not this dashboard.
      if (extraCopies > 0) {
        out.push({
          kind: 'complete-with-extras',
          set_slug: setSlug,
          set_name: info.name,
          set_low_sell: setLowSell,
          set_vol: setVol,
          parts: partRows,
          extras: extraCopies,
          extras_plat: extrasPlat,
          net_plat: extrasPlat,
        });
      }
    } else if (missing.length <= 2 && setLowSell > 0) {
      const missingCost = missing.reduce((s, p) => s + p.low_sell, 0);
      const net = setLowSell - missingCost;
      // Only show when flipping is meaningfully better than just selling
      // the parts you already own.
      if (net > partsLowSellSum - missingCost) {
        out.push({
          kind: 'near-complete',
          set_slug: setSlug,
          set_name: info.name,
          set_low_sell: setLowSell,
          set_vol: setVol,
          parts_low_sell: partsLowSellSum - missingCost, // value of parts you already own
          parts: partRows,
          missing,
          missing_cost: missingCost,
          net_plat: net,
        });
      }
    } else if (extraCopies > 0) {
      out.push({
        kind: 'extras',
        set_slug: setSlug,
        set_name: info.name,
        parts: partRows,
        extras: extraCopies,
        extras_plat: extrasPlat,
        net_plat: extrasPlat,
      });
    }
  }

  out.sort((a, b) => b.net_plat - a.net_plat);
  return out.slice(0, limit);
}
