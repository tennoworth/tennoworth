// The dated things that move prices, in one list.
//
// worldState publishes three of them and the app showed none: when Baro lands
// and what he brings, when a Prime Vault rotation opens, and what Darvo is
// discounting today. The first two are price shocks announced days ahead —
// an unvaulting is the single most expensive surprise in prime trading — and
// knowing about them before they land is the entire value.
//
// Event reward tables are deliberately NOT parsed here. `Goals`/`Events` vary
// by event type and several carry no usable table at all, so turning them into
// "this item is about to be given away" is a separate piece of work with its
// own failure modes. Absent beats wrong.

import type { DailyDeal, Market, OwnedRecord, VaultRotation } from './types';

export type CalendarKind = 'baro' | 'vault' | 'deal';

export interface CalendarItem {
  kind: CalendarKind;
  /** When it starts (ISO). */
  at: string;
  /** When it ends (ISO), where there is an end. */
  until?: string;
  title: string;
  detail?: string;
  /** Slugs the user holds that this event affects. Empty when nothing does,
   *  or when we cannot tell — those are different, and `affectsKnown` says
   *  which. */
  affects: string[];
  /** False when the event's contents cannot be resolved to slugs at all, so
   *  an empty `affects` must not read as "this doesn't touch you". */
  affectsKnown: boolean;
}

/** Strip everything but letters, lowercased — for comparing a Prime Vault
 *  bundle SKU (`MPVRevenantPrimeSinglePack`) against a set name
 *  ("Revenant Prime"). Both sides are normalised, so the match is a
 *  containment test on letters alone rather than a guess at DE's naming. */
function letters(s: string): string {
  return s.toLowerCase().replace(/[^a-z]/g, '');
}

/**
 * Which owned sets a Prime Vault rotation covers.
 *
 * Rotation manifests are bundle SKUs with no market slug, so the only signal
 * is the name embedded in the path. A set matches when its name's letters
 * appear in a SKU's letters — exact containment, no fuzzy scoring. Anything
 * short of that is left out, because telling somebody their Nidus Prime is
 * about to be unvaulted when it isn't is worse than saying nothing.
 */
export function vaultAffects(
  rotation: VaultRotation,
  market: Market | null | undefined,
  owned: Map<string, OwnedRecord> | null | undefined,
): string[] {
  const primes = market?.calendar?.primes;
  if (!primes || !owned) return [];

  const skus = rotation.items.map(letters);
  const heldSlugs = new Set<string>();
  for (const rec of owned.values()) heldSlugs.add(rec.slug);

  const hits: string[] = [];
  for (const [setSlug, prime] of Object.entries(primes)) {
    if (!prime?.name) continue;
    const needle = letters(prime.name);
    if (needle.length < 6) continue; // too short to match safely
    if (!skus.some((s) => s.includes(needle))) continue;

    // Held either as the assembled set or as any of its parts.
    if (heldSlugs.has(setSlug)) {
      hits.push(setSlug);
      continue;
    }
    const parts = market?.set_to_parts?.[setSlug]?.parts ?? [];
    if (parts.some((p) => heldSlugs.has(p.slug))) hits.push(setSlug);
  }
  return hits;
}

/** Rows Baro contributes: his window, plus which of his stock you already hold
 *  (those are the ones whose price his arrival will move). */
function baroItems(
  market: Market | null | undefined,
  owned: Map<string, OwnedRecord> | null | undefined,
): CalendarItem[] {
  const baro = market?.baro;
  if (!baro?.activation || !baro.expiry) return [];

  const stock = baro.inventory ?? [];
  const held = new Set<string>();
  if (owned) for (const rec of owned.values()) held.add(rec.slug);
  const affects = stock
    .map((s) => s.slug)
    .filter((slug): slug is string => !!slug && held.has(slug));

  return [
    {
      kind: 'baro',
      at: baro.activation,
      until: baro.expiry,
      title: `Baro Ki'Teer${baro.location ? ` · ${baro.location}` : ''}`,
      detail: stock.length ? `${stock.length} items` : 'manifest not published yet',
      affects,
      // Without an inventory we cannot say what it touches — which is not the
      // same as it touching nothing.
      affectsKnown: !!owned && stock.length > 0,
    },
  ];
}

function vaultItems(
  market: Market | null | undefined,
  owned: Map<string, OwnedRecord> | null | undefined,
): CalendarItem[] {
  const rotations = market?.de?.vault_rotation ?? [];
  return rotations.map((r) => ({
    kind: 'vault' as const,
    at: r.activation,
    until: r.expiry,
    title: 'Prime Vault rotation',
    detail: `${r.items.length} pack${r.items.length === 1 ? '' : 's'}`,
    affects: vaultAffects(r, market, owned),
    affectsKnown: !!owned && !!market?.calendar?.primes,
  }));
}

function dealItems(market: Market | null | undefined): CalendarItem[] {
  const deals: DailyDeal[] = market?.de?.deals ?? [];
  return deals.map((d) => ({
    kind: 'deal' as const,
    at: d.expiry,
    title: `Darvo · ${d.item}`,
    detail:
      d.discount && d.sale_price
        ? `${d.discount}% off — ${d.sale_price}p`
        : undefined,
    // Darvo sells for real-money platinum from DE's store; it never touches a
    // player's tradeable holdings, so "affects nothing" here is a fact, not a
    // gap in what we know.
    affects: [],
    affectsKnown: true,
  }));
}

/**
 * Everything dated, soonest first.
 *
 * Events already finished are dropped: a calendar that shows last week's Baro
 * teaches people to ignore it.
 */
export function buildCalendar(
  market: Market | null | undefined,
  owned: Map<string, OwnedRecord> | null | undefined,
  now: number,
): CalendarItem[] {
  const items = [...baroItems(market, owned), ...vaultItems(market, owned), ...dealItems(market)];
  return items
    .filter((i) => {
      const end = i.until ? Date.parse(i.until) : Date.parse(i.at);
      return Number.isFinite(end) && end > now;
    })
    .sort((a, b) => Date.parse(a.at) - Date.parse(b.at));
}

/** Only the rows that touch what the user holds — the notification-worthy
 *  subset. Rows whose reach is unknown are excluded rather than assumed
 *  harmless. */
export function affecting(items: CalendarItem[]): CalendarItem[] {
  return items.filter((i) => i.affectsKnown && i.affects.length > 0);
}
