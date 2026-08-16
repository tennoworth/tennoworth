// Sell-view presets: each is a one-click configuration of (filters, tag
// chips, visible columns, default sort). Casual users said the 11-column
// default table was overwhelming; presets reshape what shows so the
// workflow's signal isn't drowned in unrelated numbers.

export interface Preset {
  minPrice: number;
  hideAtLvl: number;
  typeFilter: string;
  activeTags: string[];
  label: string;
  hint: string;
  columns?: string[];
  vaultOnly?: boolean;
  ducatsOnly?: boolean;
  minVol?: number; // hard per-preset liquidity floor (Trending uses it)
  minMedian?: number; // 90d-baseline price floor — a +1100% Δ on a 1p fish is noise or wash-trading, not a mover
  /** Restrict to these row types (any of). Unlike `typeFilter` — the user's
   *  single-type dropdown — a preset can span several (Spares: Mods + Arcanes). */
  typesAny?: string[];
  /** Spares mode: rows are duplicate mods/arcanes, and "sellable" means the
   *  copies you'd otherwise dissolve (see `spareQty`), not owned − reserve. */
  sparesOnly?: boolean;
  defaultSort?: { key: string; dir: number };
}

export const PRESETS: Record<string, Preset> = {
  default: {
    minPrice: 5, hideAtLvl: 5, typeFilter: 'all', activeTags: [],
    label: 'Default', hint: 'everything sellable, best first',
    defaultSort: { key: 'sell_score', dir: -1 },
  },
  spares: {
    minPrice: 3, hideAtLvl: 11, typeFilter: 'all', activeTags: [],
    label: 'Spares', hint: 'duplicate mods & arcanes worth ≥ 3p — sell these instead of dissolving them (keeps one unless you own a ranked copy)',
    columns: ['name', 'owned', 'low_sell', 'volume_48h', 'sell_score', 'potential_plat'],
    typesAny: ['Mods', 'Arcanes'],
    sparesOnly: true,
    defaultSort: { key: 'potential_plat', dir: -1 },
  },
  ducats: {
    minPrice: 0, hideAtLvl: 11, typeFilter: 'all', activeTags: [],
    label: 'Ducats', hint: 'prime parts worth feeding to Baro (ducats = his currency)',
    columns: ['name', 'owned', 'sell_score', 'low_sell', 'volume_48h', 'ducats', 'plat_per_100d'],
    // Rank by plat-per-100-ducats ASCENDING: lowest plat value per ducat =
    // worth more fed to Baro than sold on WFM. (Nulls — non-ducat rows — sink.)
    defaultSort: { key: 'plat_per_100d', dir: 1 },
    ducatsOnly: true,
  },
  trending: {
    minPrice: 5, hideAtLvl: 5, typeFilter: 'all', activeTags: [],
    label: 'Trending', hint: 'movers vs 90d median · vol ≥ 10 · baseline ≥ 5p',
    columns: ['name', 'owned', 'sell_score', 'low_sell', 'medians_7d', 'delta_90d_pct', 'volume_48h', 'ratio'],
    defaultSort: { key: 'delta_90d_pct', dir: -1 },
    minVol: 10,
    minMedian: 5,
  },
  sets: {
    minPrice: 0, hideAtLvl: 11, typeFilter: 'all', activeTags: ['set'],
    label: 'Sets', hint: 'only set-tagged rows',
    columns: ['name', 'owned', 'sell_score', 'low_sell', 'top_buy', 'potential_plat'],
    defaultSort: { key: 'sell_score', dir: -1 },
  },
  vault: {
    minPrice: 0, hideAtLvl: 11, typeFilter: 'all', activeTags: [],
    label: 'Vaulted', hint: 'vaulted + vaulting-soon prime parts (sell before the cliff)',
    columns: ['name', 'owned', 'sell_score', 'low_sell', 'top_buy', 'volume_48h', 'potential_plat'],
    vaultOnly: true,
    defaultSort: { key: 'sell_score', dir: -1 },
  },
};

export interface PresetFilterValues {
  minPrice: number;
  hideAtLvl: number;
  typeFilter: string;
  activeTags: Set<string>;
}

/** What the raw filter `$state` should be set to when the user clicks
 * preset `name` — `null` for an unknown name. */
export function presetFilterValues(name: string): PresetFilterValues | null {
  const p = PRESETS[name];
  if (!p) return null;
  return {
    minPrice: p.minPrice,
    hideAtLvl: p.hideAtLvl,
    typeFilter: p.typeFilter,
    activeTags: new Set(p.activeTags),
  };
}

/** Whether the current hand-set filters still match preset `name` — used to
 * null out the active-preset selection the moment the user diverges from it
 * by editing a slider/dropdown/chip directly. */
export function presetStillMatches(name: string, current: PresetFilterValues): boolean {
  const p = PRESETS[name];
  if (!p) return false;
  return (
    current.minPrice === p.minPrice &&
    current.hideAtLvl === p.hideAtLvl &&
    current.typeFilter === p.typeFilter &&
    current.activeTags.size === p.activeTags.length &&
    p.activeTags.every((t) => current.activeTags.has(t))
  );
}
