// The dated things that move prices, in one list.
//
// worldState publishes three of them and the app showed none: when Baro lands
// and what he brings, when a Prime Vault rotation opens, and what Darvo is
// discounting today. The first two are price shocks announced days ahead -
// an unvaulting is the single most expensive surprise in prime trading - and
// knowing about them before they land is the entire value.
//
// Event rows are limited to fixed reward containers observed in DE's live
// Goals shape. Bounty deck references and announcement-only Events remain
// unknown; absent still beats inventing a guaranteed basket.

import type { DailyDeal, EventRewardEntry, Market, OwnedRecord, VaultRotation } from './types';

export type CalendarKind = 'baro' | 'vault' | 'deal' | 'event';
export type CalendarReach = 'scan' | 'none' | 'hits' | 'partial-hits' | 'unknown';

export interface CalendarItem {
  /** Stable upstream ID where one exists. */
  id?: string;
  kind: CalendarKind;
  /** When it starts (ISO). */
  at: string;
  /** When it ends (ISO), where there is an end. */
  until?: string;
  title: string;
  detail?: string;
  /** Slugs the user holds that this event affects. Empty when nothing does,
   *  or when we cannot tell - those are different, and `affectsKnown` says
   *  which. */
  affects: string[];
  /** False when the event's contents cannot be resolved to slugs at all, so
   *  an empty `affects` must not read as "this doesn't touch you". */
  affectsKnown: boolean;
  reach?: CalendarReach;
  stale?: boolean;
  dataAgeDays?: number;
}

/**
 * Split an identifier into lowercase words.
 *
 * Bundle SKUs are CamelCase with no separators
 * (`MPVRevenantPrimeSinglePack` → mpv, revenant, prime, single, pack); set
 * names are already spaced ("Revenant Prime" → revenant, prime). Reducing both
 * to word lists is what makes the comparison safe.
 *
 * A raw letters-only containment test - which this used to be - produces real
 * false positives, because seven prime names are letter-substrings of another:
 * Bo Prime inside Limbo Prime, Bronco Prime inside Akbronco Prime, Lex Prime
 * inside Aklex Prime, and four more. Tokenising kills all of them, since
 * "akbronco" is one word and never equals "bronco".
 */
function words(s: string): string[] {
  return (
    s
      // "&" is spelled out in DE's identifiers - Cobra & Crane Prime is
      // MPVCobraAndCranePrimeSinglePack - so dropping the symbol as
      // punctuation loses the word entirely and the two sides stop lining up.
      // Two of the 159 current prime names hit this: Cobra & Crane and
      // Silva & Aegis.
      .replace(/&/g, ' and ')
      // Acronym → word: MPVRevenant → MPV Revenant. Needed first, and easy to
      // miss - without it the leading `MPV` fuses onto the frame name and
      // nothing ever matches.
      .replace(/([A-Z]+)([A-Z][a-z])/g, '$1 $2')
      // word → Word: AkbroncoPrime → Akbronco Prime.
      .replace(/([a-z0-9])([A-Z])/g, '$1 $2')
      .toLowerCase()
      .split(/[^a-z0-9]+/)
      .filter(Boolean)
  );
}

/** Whether `needle` appears as a contiguous run of whole words in `haystack`. */
function containsWords(haystack: string[], needle: string[]): boolean {
  if (!needle.length || needle.length > haystack.length) return false;
  outer: for (let i = 0; i + needle.length <= haystack.length; i += 1) {
    for (let j = 0; j < needle.length; j += 1) {
      if (haystack[i + j] !== needle[j]) continue outer;
    }
    return true;
  }
  return false;
}

/**
 * Which owned sets a Prime Vault rotation covers.
 *
 * Rotation manifests are bundle SKUs with no market slug, so the only signal
 * is the name embedded in the path. A set matches when its name appears as a
 * contiguous run of whole WORDS in the SKU - never a raw substring, which
 * would flag a held Bronco Prime on an Akbronco Prime pack. No fuzzy scoring
 * either: telling somebody their Nidus Prime is about to be unvaulted when it
 * isn't is worse than saying nothing.
 */
export function vaultAffects(
  rotation: VaultRotation,
  market: Market | null | undefined,
  owned: Map<string, OwnedRecord> | null | undefined,
): string[] {
  const primes = market?.calendar?.primes;
  if (!primes || !owned) return [];

  const skus = rotation.items.map(words);
  const heldSlugs = new Set<string>();
  for (const rec of owned.values()) heldSlugs.add(rec.slug);

  const hits: string[] = [];
  for (const [setSlug, prime] of Object.entries(primes)) {
    if (!prime?.name) continue;
    const needle = words(prime.name);
    if (!needle.length) continue;
    if (!skus.some((sku) => containsWords(sku, needle))) continue;

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
      // Without an inventory we cannot say what it touches - which is not the
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
        ? `${d.discount}% off - ${d.sale_price}p`
        : undefined,
    // Darvo sells for real-money platinum from DE's store; it never touches a
    // player's tradeable holdings, so "affects nothing" here is a fact, not a
    // gap in what we know.
    affects: [],
    affectsKnown: true,
  }));
}

function eventItems(
  market: Market | null | undefined,
  owned: Map<string, OwnedRecord> | null | undefined,
  now: number,
): CalendarItem[] {
  const surface = market?.event_rewards;
  if (!surface) return [];
  const held = new Set<string>();
  if (owned) for (const record of owned.values()) held.add(record.slug);
  const rows = [...Object.values(surface.goals ?? {}), ...Object.values(surface.events ?? {})];
  return rows.map((event: EventRewardEntry) => {
    const stamp = market?.surface_provenance?.[`world.${event.source}s`]?.data_fetched_at;
    const stampMs = stamp ? Date.parse(stamp) : NaN;
    const dataAgeDays = Number.isFinite(stampMs)
      ? Math.max(0, Math.floor((now - stampMs) / 86_400_000))
      : undefined;
    const stale = dataAgeDays === undefined || dataAgeDays > 7;
    const slugs = new Set<string>();
    let rewardCount = 0;
    let credits = 0;
    for (const group of event.groups ?? []) {
      if (typeof group.credits === 'number' && Number.isFinite(group.credits) && group.credits > 0) {
        credits += group.credits;
      }
      for (const reward of group.rewards ?? []) {
        rewardCount += 1;
        if (reward.slug) slugs.add(reward.slug);
      }
    }
    const affects = owned ? [...slugs].filter((slug) => held.has(slug)) : [];
    const complete = event.completeness === 'complete';
    const unknown = event.completeness === 'unknown';
    const reach: CalendarReach = unknown
      ? 'unknown'
      : !owned
      ? 'scan'
      : affects.length > 0
        ? complete ? 'hits' : 'partial-hits'
        : complete ? 'none' : 'unknown';
    const rewardParts: string[] = [];
    if (rewardCount > 0) rewardParts.push(`${rewardCount} item reward${rewardCount === 1 ? '' : 's'}`);
    if (credits > 0) rewardParts.push(`${credits.toLocaleString('en-US')} credits`);
    return {
      id: event.id,
      kind: 'event' as const,
      at: event.starts_at,
      until: event.ends_at,
      title: event.title,
      detail: unknown
        ? 'fixed rewards unknown'
        : `${rewardParts.join(' · ')}${complete ? '' : ' · partial coverage'}`,
      affects,
      affectsKnown: reach === 'hits' || reach === 'none',
      reach,
      stale,
      dataAgeDays,
    };
  });
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
  const items = [
    ...baroItems(market, owned),
    ...vaultItems(market, owned),
    ...dealItems(market),
    ...eventItems(market, owned, now),
  ];
  return items
    .filter((i) => {
      const end = i.until ? Date.parse(i.until) : Date.parse(i.at);
      return Number.isFinite(end) && end > now;
    })
    .sort((a, b) => Date.parse(a.at) - Date.parse(b.at));
}

/** Only the rows that touch what the user holds - the notification-worthy
 *  subset. Rows whose reach is unknown are excluded rather than assumed
 *  harmless. */
export function affecting(items: CalendarItem[]): CalendarItem[] {
  return items.filter((i) => i.affects.length > 0 && (i.affectsKnown || i.reach === 'partial-hits'));
}
