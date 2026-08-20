// Type contracts at every boundary. The browser ingests three external
// shapes: `market.json` (server-baked snapshot), `inventory.json` (from
// the companion or DE's API), and warframestat.us `/items/` (cached in
// IndexedDB). Codifying these once lets the rest of `lib/` stay
// typed without scattering `any` everywhere.
//
// The shapes here are deliberately permissive at the edges — fields the
// scraper may not have populated yet (90d stats, vault status) are
// optional. Tight invariants get refined inside the consumer modules.

// -------- market.json --------

/** Per-slug market entry baked by `wfm-scrape build`. */
export interface MarketItemEntry {
  avg: number;
  low_sell: number;
  top_buy: number;
  vol: number;
  ratio: number;
  buys: number;
  sells: number;
  // Extended fields. Older snapshots may omit them; consumers must guard.
  tags?: string[];
  ducats?: number | null;
  low5_avg?: number; // avg of the ~5 cheapest live asks (depth-aware current price); 0/absent on older snapshots
  median_now?: number; // latest daily median ("today"); falls back to median_90d on pre-split snapshots
  median_90d?: number; // median OF the 90-day daily medians (the baseline, not "today")
  medians_7d?: number[];
  donch_top_90d?: number;
  donch_bot_90d?: number;
}

/** Component path → display info, baked from warframestat parent walk. */
interface PathInfo {
  name: string;
  slug: string;
  category: string;
}

/** Set slug → constituent parts, baked from warframestat parent walk. */
interface SetEntry {
  name: string;
  parts: Array<{ slug: string; component_name: string }>;
}

/** Single drop entry on a relic, from drops.warframestat.us. */
interface RelicReward {
  reward_slug: string;
  reward_name: string;
  rarity: string;
  chance: number;
}

/** Prime-part vault state — `vaulted` and `vaulting-soon` are sell-signals. */
export type VaultStatus = 'vaulted' | 'vaulting-soon' | 'available';

/** Baro Ki'Teer schedule, baked from warframestat at build time so the
 *  Baro view needs no runtime warframestat fetch. */
/** One line of Baro's stock. `item` is the display name — resolve it through
 *  `market.catalog` (display_name_lower → slug) to join it to prices. */
export interface BaroStock {
  item: string;
  ducats?: number;
  credits?: number;
}

interface Baro {
  activation: string;
  expiry: string;
  location: string;
  /** What he is selling. Only obtainable during his ~48h visit — the upstream
   *  endpoint returns an empty list between visits and publishes no schedule or
   *  history — so the scraper carries the last captured list forward. That
   *  means this can describe a PAST visit: compare `inventory_for` against
   *  `activation` before calling it current. Absent until his first visit
   *  after 2026-08-10, when the capture shipped. */
  inventory?: BaroStock[];
  /** The `activation` this stock was captured during. */
  inventory_for?: string;
}

/** Full market.json shape. Optional fields cover older snapshots that
 *  pre-date a feature (vault status, relic rewards, etc.). */
export interface RivenWeapon {
  name: string;
  disposition: number;
  group?: string;
  riven_type?: string;
  req_mr?: number;
  /** The weapon's in-game `/Lotus/...` path — how a scanned riven's `compat`
   *  fingerprint field resolves to this slug. */
  game_ref?: string;
}

export interface RivenDispoChange {
  slug: string;
  name: string;
  from: number;
  to: number;
  /** When the pipeline first saw the new value (ISO) — not DE's patch time. */
  seen_at: string;
}

/** One stat of WFM's `/riven/attributes` manifest, keyed by the fingerprint's
 *  DE tag (`game_ref`). `unit === 'percent'` stats display ×100. */
export interface RivenAttribute {
  game_ref: string;
  slug: string;
  name: string;
  unit?: string;
}

export interface RivenSurface {
  weapons?: Record<string, RivenWeapon>;
  changes?: RivenDispoChange[];
  attributes?: RivenAttribute[];
}

/** One price band from DE's weekly riven stats file, per weapon × reroll-state. */
export interface RivenStatTier {
  avg: number;
  median: number;
  min: number;
  max: number;
  stddev: number;
  pop: number;
}

/** DE weekly riven prices (`market.riven_stats`), keyed by WFM slug. Only the
 *  reroll-states DE actually observed appear. */
export interface RivenStatsSurface {
  [slug: string]: {
    name: string;
    unrolled?: RivenStatTier;
    rolled?: RivenStatTier;
  };
}

export interface Market {
  updated_at: string;
  platform: string;
  item_count: number;
  catalog_count: number;
  catalog: Record<string, string>;
  items: Record<string, MarketItemEntry>;
  partial?: boolean;
  path_to_info?: Record<string, PathInfo>;
  set_to_parts?: Record<string, SetEntry>;
  relic_rewards?: Record<string, RelicReward[]>;
  vault_status?: Record<string, VaultStatus>;
  baro?: Baro | null;
  /** Riven weapon dispositions from WFM's manifest + a rolling 90-day change
   *  log the pipeline diffs on every run. DE only raises dispositions now, so
   *  each change is a one-sided price event for that weapon's rivens. */
  rivens?: RivenSurface | null;
  /** DE's weekly riven price bands per weapon × reroll-state (see
   *  `fetch_riven_stats` in wfm-scrape). Absent on older snapshots. */
  riven_stats?: RivenStatsSurface | null;
  source?: string;
  // Per-surface fetch timestamps (ISO). On a CSV-only rebuild these can lag
  // `updated_at` — prices refreshed but the vendor surfaces (baro/relics/
  // vault/sets) did not. Lets the UI flag a stale schedule/vault surface.
  surface_fetched_at?: Record<string, string>;
}

// -------- inventory.json --------

/** A leveled mod instance from `Upgrades[]`. UpgradeFingerprint is a
 *  JSON STRING (not an object) — we parse `lvl` defensively. */
export interface InventoryUpgrade {
  ItemType: string;
  UpgradeFingerprint?: string;
  ItemId?: { $oid: string };
}

/** A stack entry from RawUpgrades / MiscItems / Suits / etc. Instance
 *  categories (Suits, LongGuns, Pistols, Melee, SpaceGuns, SpaceMelee,
 *  Sentinels, SentinelWeapons) carry `XP` per array element (one owned
 *  copy) instead of `ItemCount`; any XP > 0 makes that copy untradeable
 *  in-game. Stack categories have `ItemCount` and no `XP`. */
export interface InventoryStackEntry {
  ItemType?: string;
  Type?: string;
  ItemCount?: number;
  XP?: number;
}

/** Top-level inventory shape. The companion / DE's API emits ~200 keys;
 *  we only assert on the ones we read. */
export interface Inventory {
  Upgrades?: InventoryUpgrade[];
  RawUpgrades?: InventoryStackEntry[];
  MiscItems?: InventoryStackEntry[];
  Recipes?: InventoryStackEntry[];
  Suits?: InventoryStackEntry[];
  LongGuns?: InventoryStackEntry[];
  Pistols?: InventoryStackEntry[];
  Melee?: InventoryStackEntry[];
  SpaceGuns?: InventoryStackEntry[];
  SpaceMelee?: InventoryStackEntry[];
  Sentinels?: InventoryStackEntry[];
  SentinelWeapons?: InventoryStackEntry[];
  // Open shape — many other keys exist but we don't read them.
  [k: string]: unknown;
}

// -------- wfstat-catalog.json (baked from warframestat.us at build time) --------

/** Slim per-item info we cache in IndexedDB (key `wfstat-items-v3`). */
export interface SlimItemInfo {
  name: string;
  category: string | null;
}

/** Resolver output for a single `/Lotus/...` path. */
export interface ResolvedItem {
  name: string | null;
  slug: string | null;
  category: string | null;
  subtype: string | null;
}

// -------- App-internal --------

/** A resolved owned record. Keyed by composite `${slug}|${subtype ?? ''}`
 *  in the owned Map so each relic refinement is its own row. */
export interface OwnedRecord {
  count: number;
  name: string;
  type: string;
  slug: string;
  subtype: string | null;
  /** Highest `lvl` seen across instances of this item in `Upgrades`.
   *  `null` = no individualised instance at all (always show). */
  kept_lvl: number | null;
  /** Count of owned instances with XP > 0 — copies Warframe has flagged
   *  untradeable because they've been leveled. 0 for stack categories
   *  (MiscItems, Recipes, RawUpgrades), which have no per-instance XP. */
  leveled: number;
}

// -------- Desktop transport (Tauri IPC) --------

/** `/health` response shape. `assistant` was the browser companion's advisor
 *  flag; the desktop keeps the field for wire-compat but nothing surfaces it
 *  (the assistant is deliberately dormant). */
export interface PingResponse {
  ok: boolean;
  platform?: string;
  assistant?: boolean;
}

/** Plan items submitted to wfm-core via `submit_plan`. */
export interface PlanItemInput {
  slug: string;
  platinum: number;
  quantity: number;
  order_type: 'sell' | 'buy';
  visible: boolean;
  rank?: number;
  subtype?: string;
  reference_low_sell?: number;
}

/** PATCH fields for a single order (price / quantity / visible / rank). */
export interface OrderPatch {
  platinum?: number;
  quantity?: number;
  visible?: boolean;
  rank?: number;
}

/** Single per-item result echoed by the companion's POST /plan / PATCH /order. */
export interface ItemResult {
  slug: string;
  status: 'ok' | 'skipped' | 'error';
  message?: string | null;
  order_id?: string | null;
  /** 'created' | 'updated' — how an ok row landed on WFM (absent on errors
   *  and on pre-reconcile companions). */
  action?: 'created' | 'updated' | null;
}

export interface PlanResponse {
  plan_id: string;
  results: ItemResult[];
}

/** Pending-plan persistence shape — kept on disk in `pending_plan.json`. */
interface PendingPlanItem {
  slug: string;
  platinum: number;
  quantity: number;
  order_type: 'sell' | 'buy';
  visible: boolean;
  rank?: number | null;
  subtype?: string | null;
  reference_low_sell?: number | null;
  status: 'pending' | 'ok' | 'error';
  message?: string | null;
  order_id?: string | null;
}

export interface PendingPlan {
  plan_id: string;
  started_at: string;
  items: PendingPlanItem[];
}
