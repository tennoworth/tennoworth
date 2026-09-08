import type { PingResponse, PlanItemInput, OrderPatch, PendingPlan, PlanResponse, ItemResult, Market, OverlaySettings, OverlayStatus } from './data';
import type { History } from '../domain/history';

/**
 * Result of a desktop market refresh. `updated` is true only when a validated
 * 200 delivered a strictly-considerable snapshot in `market` (the caller decides
 * whether to swap, guarding a server rollback by comparing `updated_at`). On 304
 * / offline / error it is false with no `market` - the caller keeps what it has.
 * `updatedAt` reports the freshest snapshot the desktop now holds (fetched or
 * cached) so the staleness indicator stays correct even when nothing changed.
 */
export interface MarketRefreshResult {
  updated: boolean;
  updatedAt: string | null;
  etag: string | null;
  market?: Market;
}

/**
 * The operations the app performs against wfm-core. The desktop shell provides these capabilities through Tauri IPC.
 */
/** Result of the scan-broke report: the URL, and whether a browser opened. */
export interface ScanReport {
  url: string;
  opened: boolean;
}

export interface MarketCapability {
  loadCachedMarket(): Promise<Market | null>;
  refreshMarket(): Promise<MarketRefreshResult>;
  loadHistory(): Promise<History | null>;
}

export interface InventoryCapability {
  health(timeoutMs?: number): Promise<PingResponse>;
  fetchInventory(): Promise<unknown>;
  reportScanIssue(error: string | null): Promise<ScanReport>;
}

export interface OrderCapability {
  submitPlan(items: PlanItemInput[]): Promise<PlanResponse>;
  getPendingPlan(): Promise<PendingPlan | null>;
  resumePendingPlan(): Promise<PlanResponse>;
  discardPendingPlan(): Promise<unknown>;
  fetchOrders(): Promise<unknown>;
  updateOrder(orderId: string, patch: OrderPatch): Promise<unknown>;
  deleteOrder(orderId: string): Promise<unknown>;
  bulkVisibility(orderIds: string[], visible: boolean): Promise<{ results: ItemResult[] }>;
}

export interface OverlayCapability {
  getOverlaySettings(): Promise<OverlaySettings>;
  updateOverlaySettings(settings: OverlaySettings): Promise<OverlaySettings>;
  overlayStatus(): Promise<OverlayStatus>;
  setupOverlayCapture(): Promise<OverlayStatus>;
  previewRelicOverlay(): Promise<void>;
  scanOverlayNow(): Promise<void>;
  openOverlayDiagnostics(): Promise<void>;
  clearOverlayDiagnostics(): Promise<void>;
}

export interface DesktopCapabilities extends MarketCapability, InventoryCapability, OrderCapability, OverlayCapability { }

// `withGlobalTauri: true` injects `window.__TAURI__` (the public API surface,
// with `.core.invoke`); `__TAURI_INTERNALS__` is the lower-level object the
// runtime sniff keys off. Prefer the public core.invoke, fall back to the
// internals shim.
export type TauriInvoke = <T>(cmd: string, args?: Record<string, unknown>) => Promise<T>;

// ---- Desktop-only WFM auth ops --------------------------------------------
// Not on the DesktopCapabilities interface: the hosted build has no login surface, so
// these are reachable only from desktop-gated UI. Secrets flow webview → Rust
// exactly once per call and are never returned.

export interface DesktopWfmStatus {
  /** An encrypted login envelope exists on disk. */
  logged_in: boolean;
  /** The desktop process holds the decrypted JWT in memory. */
  unlocked: boolean;
}

// ---- Live top-of-book prices (desktop only; public WFM v2 endpoint) ----

export interface LiveTopQuery {
  slug: string;
  rank?: number | null;
  subtype?: string | null;
}

export interface LiveTop {
  slug: string;
  rank?: number | null;
  subtype?: string | null;
  /** ≤5 best online asks, cheapest first. */
  sells: number[];
  /** ≤5 best online bids, highest first. */
  buys: number[];
  low_sell: number | null;
  top_buy: number | null;
  /** Your own order on this tier, if the desktop knew your username and it was
   *  among the top ≤5 - excluded from `sells`/`buys`, so `low_sell` is the
   *  best ask that is NOT yours. */
  own_ask?: number | null;
  own_bid?: number | null;
  /** Set when this one lookup failed; the row simply has no live data. */
  error?: string | null;
  buyer_book?: {
    orders: BuyerOrder[];
    own_orders_excluded: boolean;
    observed_at: string | null;
  } | null;
}

export interface BuyerOrder {
  id: string;
  user_id: string;
  name: string;
  user_slug: string;
  status: string;
  platform: string;
  crossplay: boolean;
  quantity: number;
  per_trade: number;
  /** Total price for one complete lot. */
  platinum: number;
}

// ---- Riven auction comps (desktop only; WFM v1 auctions search) ----

export interface RivenAuctionAttribute {
  url_name: string;
  value: number;
  positive: boolean;
}

export interface RivenAuction {
  id: string;
  /** Effective ask: buyout for direct sells, else the starting bid. */
  price: number;
  buyout_price: number | null;
  starting_price: number;
  top_bid: number | null;
  is_direct_sell: boolean;
  owner: string | null;
  owner_status: string | null;
  mod_rank: number;
  mastery_level: number;
  re_rolls: number;
  polarity: string | null;
  name: string | null;
  platform: string | null;
  attributes: RivenAuctionAttribute[];
}

// ---- Price watches (desktop only) ----

export interface Watch {
  id: number;
  slug: string;
  name: string;
  subtype: string | null;
  rank: number | null;
  /** 'sell' = fires when the lowest other ask ≤ threshold; 'buy' = when the highest other bid ≥ threshold. */
  side: 'sell' | 'buy';
  threshold: number;
  created_at: string;
  last_price: number | null;
  /** Unix seconds. */
  last_checked_at: number | null;
  /** Unix seconds. */
  last_fired_at: number | null;
}

export interface NewWatch {
  slug: string;
  name: string;
  subtype?: string | null;
  rank?: number | null;
  side: 'sell' | 'buy';
  threshold: number;
}

export interface WatchOutcome {
  id: number;
  slug: string;
  name: string;
  side: 'sell' | 'buy';
  threshold: number;
  price: number | null;
  satisfied: boolean;
  fire: boolean;
}

// ---- Trade ledger (desktop only; EE.log detection) ----

export interface TradeItem {
  name: string;
  qty: number;
  direction: 'given' | 'received';
}

export interface TradeRow {
  id: number;
  /** Unix seconds. */
  at: number;
  partner: string;
  kind: 'sale' | 'purchase' | 'trade';
  plat: number;
  items: TradeItem[];
  log_stamp: string | null;
  /** A WFM listing was adjusted after this trade. */
  wfm_closed: boolean;
}

export interface EeLogStatus {
  /** EE.log path being tailed, or null when the game's log wasn't found. */
  path: string | null;
  auto_close: boolean;
}

export interface TradeDetected {
  id: number;
  trade: { partner: string; kind: TradeRow['kind']; plat: number; items: TradeItem[]; log_stamp: string | null };
  /** [item name, new listing quantity (0 = deleted)] */
  adjusted: Array<[string, number]>;
}

export type NotificationCategory = typeof NOTIFICATION_CATEGORIES[number];

export type NotificationTarget = 'sell' | 'orders' | 'ledger' | 'watches' | 'baro' | 'routines';

export interface NotificationEntry {
  id: number; category: NotificationCategory; title: string; body: string;
  target: NotificationTarget; created_at: number; read: boolean;
  delivery: 'pending' | 'sent' | 'failed' | 'inbox_only';
}

export interface NotificationPreferences {
  popups: boolean;
  categories: Record<NotificationCategory, { enabled: boolean; native: boolean }>;
}

export const NOTIFICATION_CATEGORIES = ['trades', 'watches', 'scans', 'baro', 'calendar', 'digest'] as const;
