// Riven extraction + resolution. The scan hands the SPA the raw DE inventory
// JSON; rivens live in `Upgrades[]` under /Lotus/Upgrades/Mods/Randomized/*
// with the weapon + stats inside the UpgradeFingerprint JSON string (the same
// shape extractKeptLvls reads for its lvl, just parsed fully here).

import type {
  Inventory,
  Market,
  RivenAttribute,
  RivenDispoChange,
  RivenStatTier,
  RivenSurface,
  RivenWeapon,
} from './types';

/** One stat line of a riven fingerprint — DE's raw int32 value, which encodes
 *  the multiplier as value × 2^30 (see rivenStatMultiplier). */
export interface RivenFingerprintStat {
  tag: string;
  value: number;
}

/** A riven parsed from the inventory's `Upgrades[]`. `slug`/`weaponName` are
 *  filled by `resolveRivens` once the market's rivens surface is available. */
export interface OwnedRiven {
  /** The full /Lotus/Upgrades/Mods/Randomized/... path. */
  path: string;
  /** Weapon /Lotus/... path from the fingerprint; null on veiled rivens. */
  compat: string | null;
  /** WFM weapon slug, resolved via the weapons manifest's `game_ref`. */
  slug: string | null;
  /** WFM weapon display name once resolved. */
  weaponName: string | null;
  rerolls: number;
  lvl: number;
  pol: string | null;
  buffs: RivenFingerprintStat[];
  curses: RivenFingerprintStat[];
  /** A challenge instead of stats — the riven is veiled. */
  veiled: boolean;
}

interface RivenFingerprint {
  compat?: string;
  buffs?: RivenFingerprintStat[];
  curses?: RivenFingerprintStat[];
  rerolls?: number;
  lvl?: number;
  pol?: string;
  challenge?: unknown;
}

const RIVEN_PATH_PREFIX = '/Lotus/Upgrades/Mods/Randomized/';

/** DE stores riven stat values as Q30 fixed-point (value × 2^30); dividing
 *  recovers the multiplier (percent stats display ×100). */
export function rivenStatMultiplier(raw: number): number {
  return raw / 1073741824;
}

function parseFingerprint(raw: string | null | undefined): RivenFingerprint {
  if (!raw) return {};
  try {
    const parsed = JSON.parse(raw) as RivenFingerprint;
    return parsed && typeof parsed === 'object' ? parsed : {};
  } catch {
    // A malformed fingerprint reads as an empty riven (veiled, unlisted) —
    // better than crashing the whole inventory view over one bad mod.
    return {};
  }
}

// DE's fingerprint stat objects use capitalized keys (\`Tag\` / \`Value\`);
// normalise to the lowercase shape the view consumes.
function statOf(s: unknown): RivenFingerprintStat | null {
  if (!s || typeof s !== 'object') return null;
  const o = s as Record<string, unknown>;
  const tag = o.Tag ?? o.tag;
  const value = o.Value ?? o.value;
  if (typeof tag !== 'string' || typeof value !== 'number') return null;
  return { tag, value };
}

/** Extract owned rivens from the raw DE inventory. Only `Upgrades[]` entries
 *  under the Randomized path count; everything else (railjack avionics,
 *  regular mods) is not a riven. */
export function extractRivens(inv: Inventory | null | undefined): OwnedRiven[] {
  const out: OwnedRiven[] = [];
  const ups = inv?.Upgrades;
  if (!Array.isArray(ups)) return out;
  for (const e of ups) {
    const path = e?.ItemType;
    if (typeof path !== 'string' || !path.startsWith(RIVEN_PATH_PREFIX)) continue;
    const fp = parseFingerprint(e?.UpgradeFingerprint);
    const buffs = Array.isArray(fp.buffs) ? fp.buffs.map(statOf).filter((s): s is RivenFingerprintStat => s !== null) : [];
    const curses = Array.isArray(fp.curses) ? fp.curses.map(statOf).filter((s): s is RivenFingerprintStat => s !== null) : [];
    const compat = typeof fp.compat === 'string' && fp.compat ? fp.compat : null;
    out.push({
      path,
      compat,
      slug: null,
      weaponName: null,
      rerolls: typeof fp.rerolls === 'number' ? fp.rerolls : 0,
      lvl: typeof fp.lvl === 'number' ? fp.lvl : 0,
      pol: typeof fp.pol === 'string' && fp.pol ? fp.pol : null,
      buffs,
      curses,
      veiled: compat === null && buffs.length === 0,
    });
  }
  return out;
}

/** A weapon as the Rivens view needs it, resolved from the manifest. */
export interface ResolvedRivenWeapon {
  slug: string;
  name: string;
  disposition: number;
}

/** `game_ref → weapon` index over `market.rivens.weapons` — the one join the
 *  whole view uses, built once per render. */
export function buildWeaponIndex(
  surface: RivenSurface | null | undefined,
): Map<string, ResolvedRivenWeapon> {
  const idx = new Map<string, ResolvedRivenWeapon>();
  const weapons = surface?.weapons;
  if (!weapons) return idx;
  for (const slug of Object.keys(weapons)) {
    const w: RivenWeapon | undefined = weapons[slug];
    if (w?.game_ref) idx.set(w.game_ref, { slug, name: w.name, disposition: w.disposition });
  }
  return idx;
}

/** Fill `slug` / `weaponName` for each owned riven from the market's rivens
 *  surface. Rivens whose weapon WFM doesn't list (brand-new content) stay
 *  unresolved and the view shows them without a band or comps. */
export function resolveRivens(
  rivens: OwnedRiven[],
  market: Market | null | undefined,
): OwnedRiven[] {
  const idx = buildWeaponIndex(market?.rivens);
  return rivens.map((r) => {
    if (!r.compat || r.veiled) return r;
    const w = idx.get(r.compat);
    if (!w) return r;
    return { ...r, slug: w.slug, weaponName: w.name };
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
  const preferred = r.rerolls > 0 ? entry.rolled : entry.unrolled;
  const other = r.rerolls > 0 ? entry.unrolled : entry.rolled;
  return preferred ?? other ?? null;
}

/** The most recent disposition change for a weapon, from the rolling log. */
export function dispoChangeFor(
  slug: string | null,
  surface: RivenSurface | null | undefined,
): RivenDispoChange | undefined {
  if (!slug) return undefined;
  return surface?.changes?.find((c) => c.slug === slug);
}

/** The stat's display name + unit from WFM's attributes manifest, by the DE
 *  tag the fingerprint uses (`game_ref`). */
export function attributeForTag(
  tag: string,
  attrs: RivenAttribute[] | undefined,
): RivenAttribute | undefined {
  return attrs?.find((a) => a.game_ref === tag);
}

/** Human stat line: "+95.3% critical damage" for a percent stat, else the raw
 *  multiplier ("+1.50 punch through"). `positive` is whether the line is a
 *  buff (fingerprint buffs are positive, curses negative). */
export function formatRivenStat(
  tag: string,
  raw: number,
  positive: boolean,
  attrs: RivenAttribute[] | undefined,
): string {
  const attr = attributeForTag(tag, attrs);
  // Curses store their raw value ALREADY negative in the fingerprint, so the
  // sign comes from the magnitude via abs() and the buff/curse flag — never
  // both, or a curse renders "--27.9%".
  const mult = Math.abs(rivenStatMultiplier(raw));
  const sign = positive ? '+' : '-';
  if (attr?.unit === 'percent') {
    return sign + (mult * 100).toFixed(1) + '% ' + attr.name;
  }
  return sign + mult.toFixed(2) + ' ' + (attr?.name ?? tag);
}

/** DE's internal polarity codes → the glyph riven tools use. The full table:
 *  AP_ATTACK=Madurai(V), AP_DEFENSE=Vazarin(D), AP_TACTIC=Naramon(—),
 *  AP_REGEN=Zenurik(Y), AP_NARAMON=Unairu(U), AP_PENJAGA(P), AP_UMBRA(◈). */
const POLARITY_SYMBOLS: Record<string, string> = {
  AP_ATTACK: 'V',
  AP_DEFENSE: 'D',
  AP_TACTIC: '—',
  AP_REGEN: 'Y',
  AP_NARAMON: 'U',
  AP_PENJAGA: 'P',
  AP_UMBRA: '◈',
};

export function polaritySymbol(pol: string | null): string {
  if (!pol) return '';
  return POLARITY_SYMBOLS[pol] ?? pol;
}

/** Human stat line for a WFM auction attribute — the auction's `value` is
 *  already converted (multiplier scale), so only the percent ×100 applies. */
export function formatAuctionStat(
  urlName: string,
  value: number,
  positive: boolean,
  attrs: RivenAttribute[] | undefined,
): string {
  const attr = attrs?.find((a) => a.slug === urlName);
  // Same single-sign rule as formatRivenStat: WFM quotes negative stats with
  // the sign already on the value.
  const mag = Math.abs(value);
  const sign = positive ? '+' : '-';
  if (attr?.unit === 'percent') {
    return sign + (mag * 100).toFixed(1) + '% ' + attr.name;
  }
  return sign + mag.toFixed(2) + ' ' + (attr?.name ?? urlName);
}
