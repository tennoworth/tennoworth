// Riven rules the Rivens view applies. Extraction from the DE inventory, the
// weapon join, the band choice, the stat join, splicing, the comps filter and
// the similarity score are native (market_domain::rivens, documented there)
// and run from the view module (native.ts); this file is their call shape plus
// the display the view prints: stat labels, auction stat lines, polarity
// glyphs and the copyable bug report.

import type { Inventory, Market, RivenAttribute, RivenDispoChange, RivenStatTier, RivenSurface } from '../contracts/data';
import type { OwnedRiven, RivenStatFilterPlan, SpliceRecipe } from '../contracts/generated/web';
import { call, native } from './native';

export type { OwnedRiven };

/** Owned rivens from the raw DE inventory's `Upgrades[]`. */
export function extractRivens(inv: Inventory | null | undefined): OwnedRiven[] {
  return call({ op: 'extract_rivens', input: { upgrades: inv?.Upgrades ?? null } });
}

/** Fill `slug` / `weaponName` for each owned riven from the market's rivens
 *  surface. Rivens whose weapon WFM doesn't list (brand-new content) stay
 *  unresolved and the view shows them without a band or comps. */
export function resolveRivens(rivens: OwnedRiven[], market: Market | null | undefined): OwnedRiven[] {
  if (rivens.length === 0) return rivens;
  const weapons = Object.entries(market?.rivens?.weapons ?? {});
  const picked = call({ op: 'resolve_rivens', input: {
    rivens: rivens.map(r => ({ compat: r.compat, veiled: r.veiled })),
    game_refs: weapons.map(([, w]) => w?.game_ref ?? null),
  } });
  return rivens.map((r, i) => {
    const at = picked[i];
    const hit = at == null ? undefined : weapons[at];
    return hit ? { ...r, slug: hit[0], weaponName: hit[1].name } : r;
  });
}

/** The DE weekly band for a riven: the rerolled tier when it has rerolls,
 *  else unrolled; falls back to the other tier when DE only published one. */
export function bandForRiven(
  r: Pick<OwnedRiven, 'slug' | 'rerolls'>,
  stats: Market['riven_stats'],
): RivenStatTier | null {
  if (!r.slug) return null;
  const entry = stats?.[r.slug];
  if (!entry) return null;
  const tier = native().web_riven_band(r.rerolls, Number(entry.unrolled != null), Number(entry.rolled != null));
  return (tier === 2 ? entry.rolled : tier === 1 ? entry.unrolled : null) ?? null;
}

/** The most recent disposition change for a weapon: the rolling log is
 *  newest first, so this is a keyed read rather than a rule. */
export function dispoChangeFor(
  slug: string | null,
  surface: RivenSurface | null | undefined,
): RivenDispoChange | undefined {
  if (!slug) return undefined;
  return surface?.changes?.find((c) => c.slug === slug);
}

/** One fingerprint stat joined to the manifest. `slug` is null when WFM's
 *  manifest has no entry for the tag. */
export interface ResolvedRivenStat {
  tag: string;
  slug: string | null;
  positive: boolean;
  label: string;
  spliced: boolean;
}

export interface SpliceOption {
  recipe: SpliceRecipe;
  /** The ingredients as they sit on the riven, in recipe order. */
  consumed: readonly [ResolvedRivenStat, ResolvedRivenStat];
  /** DE: "if one of the consumed traits was a negative bonus, the random
   *  trait will also be a negative". */
  randomIsNegative: boolean;
}

export interface RivenRowRead {
  stats: ResolvedRivenStat[];
  splices: SpliceOption[];
  /** A comps filter on the riven's positive stats, or null when none resolve. */
  filter: RivenStatFilterPlan | null;
  /** Fingerprint keys the app does not read yet, reported rather than guessed at. */
  unread: string[];
}

export interface RivenRowSource {
  riven: Pick<OwnedRiven, 'buffs' | 'curses' | 'raw' | 'veiled'>;
  /** The weapon's WFM `riven_type`, which picks the splice recipes. */
  rivenType?: string | null;
}

/** A stat the manifest cannot name keeps its raw tag, marked, so a new DE
 *  trait is visible as unknown instead of passing for a real stat name. */
function statLabel(tag: string, positive: boolean, name: string | null): string {
  const sign = positive ? '+' : '-';
  return name != null ? sign + name : `${sign}${tag} (unrecognised)`;
}

/** The rows of the Rivens view, in one native call for the page on screen. */
export function readRivenRows(rows: readonly RivenRowSource[], attrs: RivenAttribute[] | undefined): RivenRowRead[] {
  if (rows.length === 0) return [];
  const read = call({ op: 'riven_rows', input: {
    attributes: attrs ?? [],
    rivens: rows.map(({ riven, rivenType }) => ({
      buffs: riven.buffs, curses: riven.curses, raw: riven.raw, veiled: riven.veiled, riven_type: rivenType ?? undefined,
    })),
  } });
  return read.map((row) => {
    const stats = row.stats.map((s) => ({ tag: s.tag, slug: s.slug, positive: s.positive, label: statLabel(s.tag, s.positive, s.name), spliced: s.spliced }));
    return {
      stats,
      splices: row.splices.map((o) => ({ recipe: o.recipe, consumed: [stats[o.consumed[0]]!, stats[o.consumed[1]]!] as const, randomIsNegative: o.randomIsNegative })),
      filter: row.filter,
      unread: row.unread,
    };
  });
}

let splicedTraits: ReadonlySet<string> | null = null;

/** Spliced traits exist only through splicing, so a listing or fingerprint
 *  carrying one is a spliced riven; WFM marks nothing else. */
export function isSplicedStat(slug: string): boolean {
  splicedTraits ??= new Set(call({ op: 'spliced_traits', input: null }));
  return splicedTraits.has(slug);
}

/**
 * The riven as DE describes it, for a user to paste into a bug report: the
 * mod path and the fingerprint, nothing else from the inventory entry (no
 * item id). The fingerprint is re-indented when it parses and passed through
 * verbatim when it does not, so a malformed one is still reportable.
 */
export function rivenReport(riven: Pick<OwnedRiven, 'path' | 'raw'>): string | null {
  if (riven.raw == null) return null;
  let fingerprint: unknown = riven.raw;
  try {
    fingerprint = JSON.parse(riven.raw);
  } catch {
    // keep the string
  }
  return JSON.stringify({ ItemType: riven.path, UpgradeFingerprint: fingerprint }, null, 2);
}

/** DE's internal polarity codes → the glyph riven tools use. The full table:
 *  AP_ATTACK=Madurai(V), AP_DEFENSE=Vazarin(D), AP_TACTIC=Naramon(-),
 *  AP_REGEN=Zenurik(Y), AP_NARAMON=Unairu(U), AP_PENJAGA(P), AP_UMBRA(◈). */
const POLARITY_SYMBOLS: Record<string, string> = {
  AP_ATTACK: 'V',
  AP_DEFENSE: 'D',
  AP_TACTIC: '-',
  AP_REGEN: 'Y',
  AP_NARAMON: 'U',
  AP_PENJAGA: 'P',
  AP_UMBRA: '◈',
};

export function polaritySymbol(pol: string | null): string {
  if (!pol) return '';
  return POLARITY_SYMBOLS[pol] ?? pol;
}

/** Human stat line for a WFM auction attribute. WFM already sends percent
 *  values in display units (`83.1` means `83.1%`), unlike inventory Q30. */
export function formatAuctionStat(
  urlName: string,
  value: number,
  positive: boolean,
  attrs: RivenAttribute[] | undefined,
): string {
  const attr = attrs?.find((a) => a.slug === urlName);
  const name = attr?.name ?? urlName;
  // Faction damage is a multiplier: WFM sends 1.43 for x1.43 and 0.7 for the
  // x0.70 curse, so a +/- sign on it would misstate the effect.
  if (attr?.unit === 'multiply') return '×' + value.toFixed(2) + ' ' + name;
  // One sign, from `positive`: WFM quotes negative stats with the sign
  // already on the value.
  const mag = Math.abs(value);
  const sign = positive ? '+' : '-';
  if (attr?.unit === 'percent') return sign + mag.toFixed(1) + '% ' + name;
  if (attr?.unit === 'seconds') return sign + mag.toFixed(1) + ' s ' + name;
  return sign + mag.toFixed(2) + ' ' + name;
}

interface AuctionStatLike {
  url_name: string;
  positive: boolean;
}

/** Signed-stat similarity (0-100) of the riven to each auction, in one native
 *  call per comps sample; null where no stats can be compared. */
export function rivenSimilarities(
  riven: Pick<OwnedRiven, 'buffs' | 'curses'>,
  auctions: readonly (readonly AuctionStatLike[])[],
  attrs: RivenAttribute[] | undefined,
): (number | null)[] {
  if (auctions.length === 0) return [];
  return call({ op: 'riven_similarity', input: {
    attributes: attrs ?? [],
    buffs: riven.buffs,
    curses: riven.curses,
    auctions: auctions.map((stats) => stats.map((s) => ({ url_name: s.url_name, positive: s.positive }))),
  } });
}
