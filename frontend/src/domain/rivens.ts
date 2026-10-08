// Riven extraction + resolution. The scan hands the SPA the raw DE inventory
// JSON; rivens live in `Upgrades[]` under /Lotus/Upgrades/Mods/Randomized/*
// with the weapon + stats inside the UpgradeFingerprint JSON string (the same
// shape extractKeptLvls reads for its lvl, just parsed fully here).

import type { RivenStatFilter } from '../contracts/desktop';
import type {
  Inventory,
  Market,
  RivenAttribute,
  RivenDispoChange,
  RivenStatTier,
  RivenSurface,
  RivenWeapon,
} from '../contracts/data';

/** One stat line of a riven fingerprint. `value` is a Q30 roll fraction used
 *  as one input to DE's final stat formula; it is not the displayed stat. */
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
  /** A challenge instead of stats - the riven is veiled. */
  veiled: boolean;
  /** DE's fingerprint string exactly as scanned. Kept because DE adds riven
   *  state (locks, splices) before anyone knows its keys, and the parsed
   *  fields above drop what they do not name. Absent on snapshots saved
   *  before it was kept. */
  raw?: string;
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

function parseFingerprint(raw: string | null | undefined): RivenFingerprint {
  if (!raw) return {};
  try {
    const parsed = JSON.parse(raw) as RivenFingerprint;
    return parsed && typeof parsed === 'object' ? parsed : {};
  } catch {
    // A malformed fingerprint reads as an empty riven (veiled, unlisted) -
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
      ...(typeof e?.UpgradeFingerprint === 'string' ? { raw: e.UpgradeFingerprint } : {}),
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

/** `game_ref → weapon` index over `market.rivens.weapons` - the one join the
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

/** Human stat name from the fingerprint. Exact values cannot be recovered
 * from the roll fraction alone: DE's formula also needs the Riven class,
 * weapon disposition, rank, stat-count weights, and per-stat base values.
 * A tag the manifest does not know keeps the raw tag, marked, so a new DE
 * trait is visible as unknown instead of passing for a real stat name. */
export function formatRivenStat(
  tag: string,
  positive: boolean,
  attrs: RivenAttribute[] | undefined,
): string {
  const attr = attributeForTag(tag, attrs);
  const sign = positive ? '+' : '-';
  return attr ? sign + attr.name : `${sign}${tag} (unrecognised)`;
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

export function resolveRivenStats(
  riven: Pick<OwnedRiven, 'buffs' | 'curses'>,
  attrs: RivenAttribute[] | undefined,
): ResolvedRivenStat[] {
  const one = (stat: RivenFingerprintStat, positive: boolean): ResolvedRivenStat => {
    const slug = attributeForTag(stat.tag, attrs)?.slug ?? null;
    return {
      tag: stat.tag,
      slug,
      positive,
      label: formatRivenStat(stat.tag, positive, attrs),
      spliced: slug != null && isSplicedStat(slug),
    };
  };
  return [...riven.buffs.map((s) => one(s, true)), ...riven.curses.map((s) => one(s, false))];
}

// ---- Riven splicing (Update 44.1) ----
//
// A Riven Splicer consumes two specific traits and writes one spliced trait
// plus a random trait in their place. Recipes are DE's published table (PC
// patch notes 44.1.0), keyed here by WFM attribute slug. Spliced traits exist
// only through splicing, so a listing or fingerprint carrying one of these
// slugs is a spliced riven; WFM marks nothing else.

/** Which riven classes a recipe applies to. Archgun rivens report `rifle`,
 *  companion weapons report the class of riven they take. */
export type SpliceScope = 'ranged' | 'melee' | 'all';

export interface SpliceRecipe {
  result: string;
  /** DE's name for the spliced trait, for when the snapshot's manifest
   *  predates it. */
  name: string;
  ingredients: readonly [string, string];
  scope: SpliceScope;
}

const DAMAGE = 'base_damage_/_melee_damage';
const ATTACK_SPEED = 'fire_rate_/_attack_speed';

export const SPLICE_RECIPES: readonly SpliceRecipe[] = [
  { result: 'weak_point_damage', name: 'Weak Point Damage', ingredients: [DAMAGE, 'zoom'], scope: 'ranged' },
  { result: 'weak_point_damage', name: 'Weak Point Damage', ingredients: [DAMAGE, 'multishot'], scope: 'ranged' },
  { result: 'weak_point_critical_chance', name: 'Weak Point Critical Chance', ingredients: ['critical_chance', 'zoom'], scope: 'ranged' },
  { result: 'weak_point_critical_chance', name: 'Weak Point Critical Chance', ingredients: ['critical_chance', 'multishot'], scope: 'ranged' },
  { result: 'ammo_efficiency', name: 'Ammo Efficiency', ingredients: ['magazine_capacity', 'reload_speed'], scope: 'ranged' },
  { result: 'ammo_efficiency', name: 'Ammo Efficiency', ingredients: ['recoil', 'ammo_maximum'], scope: 'ranged' },
  { result: 'magazine_reloaded_s_when_holstered', name: 'Magazine Reload While Holstered', ingredients: ['ammo_maximum', 'reload_speed'], scope: 'ranged' },
  { result: 'magazine_reloaded_s_when_holstered', name: 'Magazine Reload While Holstered', ingredients: ['ammo_maximum', 'magazine_capacity'], scope: 'ranged' },
  // Ranged "Damage" and melee "Melee Damage" are the same WFM attribute.
  { result: 'status_damage', name: 'Status Damage', ingredients: [DAMAGE, 'status_chance'], scope: 'all' },
  { result: 'melee_damage_on_heavy_attack', name: 'Heavy Attack Damage', ingredients: ['channeling_efficiency', 'chance_to_gain_extra_combo_count'], scope: 'melee' },
  { result: 'heavy_attack_wind_up_speed', name: 'Heavy Attack Wind Up Speed', ingredients: ['channeling_efficiency', 'combo_duration'], scope: 'melee' },
  { result: 'parry_angle', name: 'Parry Angle', ingredients: [ATTACK_SPEED, 'range'], scope: 'melee' },
  { result: 'slam_attack_damage', name: 'Slam Damage', ingredients: [DAMAGE, ATTACK_SPEED], scope: 'melee' },
  { result: 'gas', name: 'Gas', ingredients: ['toxin_damage', 'heat_damage'], scope: 'all' },
  { result: 'corrosive', name: 'Corrosive', ingredients: ['toxin_damage', 'electric_damage'], scope: 'all' },
  { result: 'viral', name: 'Viral', ingredients: ['toxin_damage', 'cold_damage'], scope: 'all' },
  { result: 'radiation', name: 'Radiation', ingredients: ['heat_damage', 'electric_damage'], scope: 'all' },
  { result: 'blast', name: 'Blast', ingredients: ['heat_damage', 'cold_damage'], scope: 'all' },
  { result: 'magnetic', name: 'Magnetic', ingredients: ['electric_damage', 'cold_damage'], scope: 'all' },
  { result: 'damage_to_orokin', name: 'Damage to Orokin', ingredients: ['damage_vs_corpus', 'damage_vs_grineer'], scope: 'all' },
  { result: 'damage_to_techrot', name: 'Damage to Techrot', ingredients: ['damage_vs_corpus', 'damage_vs_infested'], scope: 'all' },
  { result: 'damage_to_scaldra', name: 'Damage to Scaldra', ingredients: ['damage_vs_infested', 'damage_vs_grineer'], scope: 'all' },
];

const SPLICED_SLUGS: ReadonlySet<string> = new Set(SPLICE_RECIPES.map((r) => r.result));

export function isSplicedStat(slug: string): boolean {
  return SPLICED_SLUGS.has(slug);
}

/** The recipe class for a weapon's `riven_type`; null when WFM gave none or
 *  one we do not recognise, which limits a riven to the all-class recipes. */
export function spliceScopeOf(rivenType: string | undefined): 'ranged' | 'melee' | null {
  if (rivenType === 'rifle' || rivenType === 'shotgun' || rivenType === 'pistol' || rivenType === 'kitgun') return 'ranged';
  if (rivenType === 'melee' || rivenType === 'zaw') return 'melee';
  return null;
}

export interface SpliceOption {
  recipe: SpliceRecipe;
  /** The ingredients as they sit on the riven, in recipe order. */
  consumed: readonly [ResolvedRivenStat, ResolvedRivenStat];
  /** DE: "if one of the consumed traits was a negative bonus, the random
   *  trait will also be a negative". */
  randomIsNegative: boolean;
}

/**
 * The splices this riven could take right now.
 *
 * One spliced trait per riven, so a riven that already carries one has none.
 * Each recipe needs both ingredients present. DE documents a negative
 * ingredient beside a positive one (the spliced trait takes the positive
 * one's grade); two negative ingredients are undocumented, so they are not
 * offered.
 */
export function spliceOptions(
  stats: readonly ResolvedRivenStat[],
  rivenType: string | undefined,
): SpliceOption[] {
  if (stats.some((s) => s.spliced)) return [];
  const scope = spliceScopeOf(rivenType);
  const out: SpliceOption[] = [];
  for (const recipe of SPLICE_RECIPES) {
    if (recipe.scope !== 'all' && recipe.scope !== scope) continue;
    const a = stats.find((s) => s.slug === recipe.ingredients[0]);
    const b = stats.find((s) => s.slug === recipe.ingredients[1]);
    if (!a || !b || (!a.positive && !b.positive)) continue;
    out.push({ recipe, consumed: [a, b], randomIsNegative: !a.positive || !b.positive });
  }
  return out;
}

/** WFM's search form takes at most three positive stats and one negative;
 *  the native side refuses anything larger. */
export const MAX_FILTER_POSITIVE = 3;

export interface RivenStatFilterPlan {
  filter: RivenStatFilter;
  /** False when a positive stat could not be resolved to a WFM slug, so the
   *  filter is broader than the riven. */
  complete: boolean;
}

/** A comps filter on the riven's positive stats, or null when none resolve.
 *  Negatives are left out: matching them too leaves most weapons with no
 *  listings, and the stat match on each row already shows them. */
export function compsFilterFor(stats: readonly ResolvedRivenStat[]): RivenStatFilterPlan | null {
  const positives = stats.filter((s) => s.positive);
  const slugs = positives.flatMap((s) => (s.slug ? [s.slug] : []));
  if (slugs.length === 0) return null;
  return {
    filter: { positive: slugs.slice(0, MAX_FILTER_POSITIVE), negative: [] },
    complete: slugs.length === positives.length && slugs.length <= MAX_FILTER_POSITIVE,
  };
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
  // Same single-sign rule as formatRivenStat: WFM quotes negative stats with
  // the sign already on the value.
  const mag = Math.abs(value);
  const sign = positive ? '+' : '-';
  if (attr?.unit === 'percent') return sign + mag.toFixed(1) + '% ' + name;
  if (attr?.unit === 'seconds') return sign + mag.toFixed(1) + ' s ' + name;
  return sign + mag.toFixed(2) + ' ' + name;
}

interface AuctionStatLike {
  url_name: string;
  value: number;
  positive: boolean;
}

/** Signed-stat Jaccard similarity, from 0–100. This compares which effects the
 * two Rivens have, not roll strength-the inventory fingerprint alone cannot
 * supply a final display value honestly. A stat the manifest cannot name still
 * counts as one of the riven's effects, matching nothing: dropping it would
 * shrink the riven and overstate the match. */
export function rivenSimilarity(
  riven: Pick<OwnedRiven, 'buffs' | 'curses'>,
  auction: AuctionStatLike[],
  attrs: RivenAttribute[] | undefined,
): number | null {
  const owned = new Set<string>();
  const addOwned = (stat: RivenFingerprintStat, positive: boolean): void => {
    const attr = attributeForTag(stat.tag, attrs);
    owned.add(`${positive ? '+' : '-'}:${attr ? attr.slug : `?${stat.tag}`}`);
  };
  riven.buffs.forEach((stat) => addOwned(stat, true));
  riven.curses.forEach((stat) => addOwned(stat, false));

  const comparable = new Set<string>();
  for (const stat of auction) {
    comparable.add(`${stat.positive ? '+' : '-'}:${stat.url_name}`);
  }
  const keys = new Set([...owned, ...comparable]);
  if (keys.size === 0 || owned.size === 0 || comparable.size === 0) return null;
  const matches = [...owned].filter((key) => comparable.has(key)).length;
  return Math.round((matches / keys.size) * 100);
}
