// Closing the ducat gap before Baro lands.
//
// The Baro board says what his stock is worth. This says how to afford it:
// which of your spare prime parts to feed the ducat kiosk, ranked so you give
// up the least market value for the most ducats.
//
// The ranking is ducats-per-plat, and that is the whole point — a Nova Prime
// Neuroptics is 65 ducats and 18p, while a Braton Prime Receiver is 45 ducats
// and 3p. Sorting by ducats alone would tell you to scrap the Neuroptics
// first, which is exactly backwards.

import { clearingPrice, hasRealPrice } from './sell-priority';
import type { Market, OwnedRecord } from './types';

export interface ScrapCandidate {
  slug: string;
  name: string;
  /** Copies you can spare — everything past the one you keep. */
  spare: number;
  ducats: number;
  /** What one copy clears at on the market. 0 when nothing is listed. */
  plat: number;
  /** Ducats per plat of market value given up. Higher is better to scrap. */
  ducatsPerPlat: number;
  /** Ducats if you scrap every spare copy. */
  totalDucats: number;
  /** Market value you would be giving up to do that. */
  totalPlat: number;
}

/** Above this plat, a part is worth more sold than scrapped almost regardless
 *  of its ducat value — 100 ducats is the ceiling and a 20p part is a poor
 *  trade for it. Candidates above the line are still returned, flagged, so a
 *  user short on ducats can make the call themselves. */
export const KEEP_ABOVE_PLAT = 15;

/**
 * How many spare copies of a slug you hold.
 *
 * "Spare" means beyond one kept copy. The owned map is keyed
 * `${slug}|${subtype}`, so this sums across subtypes rather than doing a bare
 * `get` that would silently match nothing.
 */
export function spareCopies(owned: Map<string, OwnedRecord>, slug: string): number {
  let total = 0;
  for (const rec of owned.values()) {
    if (rec.slug === slug) total += rec.count;
  }
  return Math.max(0, total - 1);
}

/**
 * Everything you could feed the ducat kiosk, best trade first.
 *
 * Only items the snapshot gives a ducat value — that is what makes something
 * scrappable at all. A part with no market price sorts as pure ducat gain
 * (giving up nothing measurable), which is correct: nobody is buying it.
 */
export function scrapCandidates(
  owned: Map<string, OwnedRecord> | null | undefined,
  market: Market | null | undefined,
): ScrapCandidate[] {
  if (!owned || !market?.items) return [];

  const bySlug = new Map<string, { name: string; count: number }>();
  for (const rec of owned.values()) {
    const cur = bySlug.get(rec.slug);
    if (cur) cur.count += rec.count;
    else bySlug.set(rec.slug, { name: rec.name, count: rec.count });
  }

  const out: ScrapCandidate[] = [];
  for (const [slug, { name, count }] of bySlug) {
    const spare = Math.max(0, count - 1);
    if (spare === 0) continue;
    const entry = market.items[slug];
    const ducats = entry?.ducats ?? 0;
    if (!ducats || ducats <= 0) continue;

    const plat = hasRealPrice(entry) ? clearingPrice(entry) : 0;
    out.push({
      slug,
      name,
      spare,
      ducats,
      plat,
      // An unsellable part costs nothing to scrap, so it ranks first. Infinity
      // sorts correctly and never reaches the UI as a number.
      ducatsPerPlat: plat > 0 ? ducats / plat : Infinity,
      totalDucats: ducats * spare,
      totalPlat: plat * spare,
    });
  }

  return out.sort((a, b) => b.ducatsPerPlat - a.ducatsPerPlat || b.ducats - a.ducats);
}

export interface DucatPlan {
  /** What to scrap, in order, to reach the target. */
  picks: ScrapCandidate[];
  /** Ducats the picks yield. */
  ducats: number;
  /** Market value the picks give up. */
  platGivenUp: number;
  /** Still short by this many ducats, 0 when the target is reached. */
  short: number;
  /** Candidates skipped for being worth more sold than scrapped. */
  heldBack: ScrapCandidate[];
}

/**
 * Greedy fill toward a ducat target.
 *
 * Greedy is right here rather than an exact knapsack: the items are divisible
 * in practice (you scrap whole copies, and there are usually many cheap ones),
 * the target is soft, and a user is going to eyeball the list anyway. An exact
 * solve would spend real effort to move the answer by one Braton receiver.
 *
 * Parts worth more than `keepAbove` on the market are held back rather than
 * spent — but they are returned, so a user who is genuinely short can decide
 * for themselves.
 */
export function planDucats(
  candidates: ScrapCandidate[],
  target: number,
  keepAbove = KEEP_ABOVE_PLAT,
): DucatPlan {
  const picks: ScrapCandidate[] = [];
  const heldBack: ScrapCandidate[] = [];
  let ducats = 0;
  let platGivenUp = 0;

  for (const c of candidates) {
    if (ducats >= target) break;
    if (c.plat > keepAbove) {
      heldBack.push(c);
      continue;
    }
    // Take only as many copies as the target needs, not the whole stack —
    // scrapping six receivers to cover a 90-ducat gap is not a plan.
    const needed = Math.max(0, target - ducats);
    const copies = Math.min(c.spare, Math.ceil(needed / c.ducats));
    if (copies <= 0) continue;
    picks.push({
      ...c,
      spare: copies,
      totalDucats: c.ducats * copies,
      totalPlat: c.plat * copies,
    });
    ducats += c.ducats * copies;
    platGivenUp += c.plat * copies;
  }

  return {
    picks,
    ducats,
    platGivenUp,
    short: Math.max(0, target - ducats),
    heldBack,
  };
}
