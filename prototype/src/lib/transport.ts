// Transport abstraction: the SPA's single seam between "the hosted
// informational site" (which performs NO companion/order/scan operations) and
// "call wfm-core directly over Tauri IPC" (the desktop app). Selected ONCE at
// boot by sniffing the Tauri runtime - see `isDesktopRuntime()` /
// `createTransport()`.
//
// The hosted site is informational only (market data + a dropped
// inventory.json); every interactive operation - scanning, listing, orders,
// login - lives in the desktop app. So the hosted build's transport is a
// HostedTransport that no-ops the two market-cache ops and throws on anything
// else (which the hosted UI never calls). The Tauri transport invokes a
// wfm-core-backed command per op; listing/order commands reject with a typed
// {code, message} CmdError which surfaces here as DesktopCmdError -
// `needs_login` / `needs_unlock` drive the SPA's login and passphrase dialogs.

import type { PingResponse, PlanItemInput, OrderPatch, PendingPlan, PlanResponse, ItemResult, Market, OverlaySettings, OverlayStatus } from './types';
import { isHistory, type History } from './history';

/**
 * A desktop command rejection, rehydrated from the Rust CmdError
 * `{ code, message }` the invoke promise rejects with. Callers branch on
 * `code` (`needs_login` / `needs_unlock` open the auth dialogs;
 * `bad_passphrase` stays in the passphrase dialog; everything else shows
 * `message` verbatim). Never carries the JWT, passphrase, or password -
 * the Rust side guarantees that.
 */
export class DesktopCmdError extends Error {
  code: string;
  constructor(code: string, message: string) {
    super(message);
    this.name = 'DesktopCmdError';
    this.code = code;
  }
}

/** Rethrow an invoke rejection as its typed form. Rust CmdError arrives as a
 *  plain `{code, message}` object; other commands reject with strings. */
function rethrowInvoke(e: unknown): never {
  if (e && typeof e === 'object') {
    const o = e as { code?: unknown; message?: unknown };
    if (typeof o.code === 'string' && typeof o.message === 'string') {
      throw new DesktopCmdError(o.code, o.message);
    }
  }
  if (e instanceof Error) throw e;
  throw new Error(String(e));
}

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
 * The operations the app performs against wfm-core. The hosted build never
 * calls the interactive ops (its UI is informational); TauriTransport is the
 * only real implementation.
 */
/** Result of the scan-broke report: the URL, and whether a browser opened. */
export interface ScanReport {
  url: string;
  opened: boolean;
}

export interface Transport {
  /** GET /health (HTTP) or the `health` command (Tauri). */
  health(timeoutMs?: number): Promise<PingResponse>;
  /**
   * The app-data-cached market snapshot, fresher than the compile-time bundled
   * floor. Desktop-only substrate: `null` in the browser (the hosted build
   * always fetches fresh same-origin) and on a desktop first run. Never fetches.
   */
  loadCachedMarket(): Promise<Market | null>;
  /**
   * Desktop-only: conditionally refresh the market snapshot from tennoworth.app
   * (ETag / If-None-Match) via a Rust command, updating the app-data cache. A
   * pure no-op in the browser (the hosted build gets fresh data same-origin from
   * the box - it must make NO third-party fetch). Never rejects on network
   * failure; a failed refresh returns `{ updated: false }`.
   */
  refreshMarket(): Promise<MarketRefreshResult>;
  /**
   * The year-long daily price history (`history.json`, built on the box from
   * relics.run - see wfm-scrape/src/history.rs). On demand, never at boot:
   * ~1 MB gzipped and optional. Hosted: same-origin fetch. Desktop: the Rust
   * ETag cache (cached copy first, then a conditional refresh). `null` when
   * unavailable - every 1-year surface simply hides.
   */
  loadHistory(): Promise<History | null>;
  /** Memory-scan the running game and return the parsed inventory object. */
  fetchInventory(): Promise<unknown>;
  submitPlan(items: PlanItemInput[]): Promise<PlanResponse>;
  getPendingPlan(): Promise<PendingPlan | null>;
  resumePendingPlan(): Promise<PlanResponse>;
  discardPendingPlan(): Promise<unknown>;
  fetchOrders(): Promise<unknown>;
  updateOrder(orderId: string, patch: OrderPatch): Promise<unknown>;
  deleteOrder(orderId: string): Promise<unknown>;
  bulkVisibility(orderIds: string[], visible: boolean): Promise<{ results: ItemResult[] }>;
  /**
   * Desktop-only: open a prefilled "scan broke" GitHub issue in the real
   * browser and resolve with the URL. Resolving with the URL matters - if the
   * open failed the caller can still show it as copyable text rather than
   * leaving a dead button.
   */
  reportScanIssue(error: string | null): Promise<ScanReport>;
  getOverlaySettings(): Promise<OverlaySettings>;
  updateOverlaySettings(settings: OverlaySettings): Promise<OverlaySettings>;
  overlayStatus(): Promise<OverlayStatus>;
  setupOverlayCapture(): Promise<OverlayStatus>;
  previewRelicOverlay(): Promise<void>;
  scanOverlayNow(): Promise<void>;
  openOverlayDiagnostics(): Promise<void>;
  clearOverlayDiagnostics(): Promise<void>;
}

/**
 * Hosted-site transport: every interactive op is a desktop capability the
 * informational site deliberately does not have. The two market-cache ops are
 * safe no-ops (the hosted build fetches fresh same-origin); anything else
 * throws rather than pretending a capability that would require the desktop app.
 */
export class HostedTransport implements Transport {
  async getOverlaySettings(): Promise<OverlaySettings> {
    return { enabled: false, autoDetect: true, shortcut: 'Ctrl+Shift+O', scale: 1, livePrices: true, showOwned: true, diagnostics: false };
  }
  async updateOverlaySettings(): Promise<OverlaySettings> {
    throw new Error('The in-game overlay is available in the desktop app.');
  }
  async overlayStatus(): Promise<OverlayStatus> {
    return { state: 'disabled', backend: 'unsupported', presentationBackend: 'tauri-window', placement: 'side-panel', ocrReady: false };
  }
  async setupOverlayCapture(): Promise<OverlayStatus> {
    throw new Error('The in-game overlay is available in the desktop app.');
  }
  async previewRelicOverlay(): Promise<void> {
    throw new Error('The in-game overlay is available in the desktop app.');
  }
  async scanOverlayNow(): Promise<void> {
    throw new Error('The in-game overlay is available in the desktop app.');
  }
  async openOverlayDiagnostics(): Promise<void> {
    throw new Error('The in-game overlay is available in the desktop app.');
  }
  async clearOverlayDiagnostics(): Promise<void> {
    throw new Error('The in-game overlay is available in the desktop app.');
  }
  async reportScanIssue(): Promise<ScanReport> {
    throw new Error('This is the informational site - the desktop app is required for account features.');
  }
  async health(): Promise<PingResponse> {
    throw new Error('This is the informational site - the desktop app is required for account features.');
  }
  async loadCachedMarket(): Promise<Market | null> {
    return null;
  }
  async refreshMarket(): Promise<MarketRefreshResult> {
    return { updated: false, updatedAt: null, etag: null };
  }
  async loadHistory(): Promise<History | null> {
    try {
      const r = await fetch('/history.json', { cache: 'no-cache' });
      if (!r.ok) return null;
      const h = (await r.json()) as History;
      return isHistory(h) ? h : null;
    } catch {
      return null;
    }
  }
  async fetchInventory(): Promise<unknown> {
    throw new Error('This is the informational site - the desktop app is required to scan your account.');
  }
  async submitPlan(): Promise<PlanResponse> {
    throw new Error('This is the informational site - the desktop app is required to list on WFM.');
  }
  async getPendingPlan(): Promise<PendingPlan | null> {
    return null;
  }
  async resumePendingPlan(): Promise<PlanResponse> {
    throw new Error('This is the informational site - the desktop app is required to list on WFM.');
  }
  async discardPendingPlan(): Promise<unknown> {
    return null;
  }
  async fetchOrders(): Promise<unknown> {
    throw new Error('This is the informational site - the desktop app is required to manage orders.');
  }
  async updateOrder(): Promise<unknown> {
    throw new Error('This is the informational site - the desktop app is required to manage orders.');
  }
  async deleteOrder(): Promise<unknown> {
    throw new Error('This is the informational site - the desktop app is required to manage orders.');
  }
  async bulkVisibility(): Promise<{ results: ItemResult[] }> {
    throw new Error('This is the informational site - the desktop app is required to manage orders.');
  }
}

// `withGlobalTauri: true` injects `window.__TAURI__` (the public API surface,
// with `.core.invoke`); `__TAURI_INTERNALS__` is the lower-level object the
// runtime sniff keys off. Prefer the public core.invoke, fall back to the
// internals shim.
export type TauriInvoke = <T>(cmd: string, args?: Record<string, unknown>) => Promise<T>;
export function resolveInvoke(): TauriInvoke {
  const w = globalThis as unknown as {
    __TAURI__?: { core?: { invoke?: TauriInvoke } };
    __TAURI_INTERNALS__?: { invoke?: TauriInvoke };
  };
  const invoke = w.__TAURI__?.core?.invoke ?? w.__TAURI_INTERNALS__?.invoke;
  if (!invoke) throw new Error('Tauri IPC unavailable (no invoke on window).');
  return invoke;
}

/** Bridge target=_blank links out of Tauri's single webview and into the
 * system browser. The Rust command applies the final scheme/host allowlist. */
export async function desktopOpenExternalUrl(url: string): Promise<boolean> {
  return await resolveInvoke()<boolean>('open_external_url', { url });
}

export function installDesktopExternalLinkHandler(root: Document = document): () => void {
  if (!isDesktopRuntime()) return () => {};
  const onClick = (event: MouseEvent): void => {
    if (event.defaultPrevented || event.button !== 0) return;
    const element = event.target instanceof Element ? event.target : null;
    const anchor = element?.closest<HTMLAnchorElement>('a[target="_blank"]');
    if (!anchor) return;
    const url = new URL(anchor.href, location.href);
    if (url.protocol !== 'https:') return;
    event.preventDefault();
    void desktopOpenExternalUrl(url.href);
  };
  root.addEventListener('click', onClick);
  return () => root.removeEventListener('click', onClick);
}

/**
 * Tauri transport: each op is a wfm-core-backed command. The listing/order ops
 * mirror serve's HTTP routes 1:1 (submit_plan ↔ POST /plan, get_pending_plan ↔
 * GET /plan/pending, …); their rejections surface as DesktopCmdError so the
 * caller can branch on `needs_login` / `needs_unlock`.
 */
export class TauriTransport implements Transport {
  async getOverlaySettings(): Promise<OverlaySettings> {
    return await resolveInvoke()<OverlaySettings>('get_overlay_settings');
  }

  async updateOverlaySettings(settings: OverlaySettings): Promise<OverlaySettings> {
    return await resolveInvoke()<OverlaySettings>('update_overlay_settings', { settings });
  }

  async overlayStatus(): Promise<OverlayStatus> {
    return await resolveInvoke()<OverlayStatus>('overlay_status');
  }

  async setupOverlayCapture(): Promise<OverlayStatus> {
    return await resolveInvoke()<OverlayStatus>('setup_overlay_capture');
  }

  async previewRelicOverlay(): Promise<void> {
    await resolveInvoke()<void>('preview_relic_overlay');
  }

  async scanOverlayNow(): Promise<void> {
    await resolveInvoke()<void>('scan_overlay_now');
  }
  async openOverlayDiagnostics(): Promise<void> {
    await resolveInvoke()<void>('open_overlay_diagnostics');
  }
  async clearOverlayDiagnostics(): Promise<void> {
    await resolveInvoke()<void>('clear_overlay_diagnostics');
  }
  async reportScanIssue(error: string | null): Promise<ScanReport> {
    return await resolveInvoke()<ScanReport>('report_scan_issue', { error });
  }

  async health(): Promise<PingResponse> {
    return await resolveInvoke()<PingResponse>('health');
  }

  async fetchInventory(): Promise<unknown> {
    // The command returns the inventory JSON as a string (the exact bytes the
    // old CLI would write); a rejected invoke carries wfm-core's graceful
    // message (e.g. "Warframe doesn't appear to be running…").
    const json = await resolveInvoke()<string>('scan_inventory');
    return JSON.parse(json);
  }

  // The `cached_market` command returns the raw cached body (or null). Parse it
  // here; a corrupt cache (parse throws) reads as "no cache" so the caller falls
  // back to the bundled floor rather than crashing the boot.
  async loadCachedMarket(): Promise<Market | null> {
    const raw = await resolveInvoke()<string | null>('cached_market');
    if (!raw) return null;
    try {
      return JSON.parse(raw) as Market;
    } catch {
      return null;
    }
  }

  async refreshMarket(): Promise<MarketRefreshResult> {
    // The Rust command swallows all network/HTTP failures and returns a no-op
    // RefreshResult, so this rejects only on a genuine IPC fault. `body` is
    // present only when `updated`; parse it into the Market to swap in.
    const r = await resolveInvoke()<{
      updated: boolean;
      updated_at: string | null;
      etag: string | null;
      body: string | null;
    }>('refresh_market');
    const market = r.updated && r.body ? (JSON.parse(r.body) as Market) : undefined;
    return { updated: !!r.updated, updatedAt: r.updated_at ?? null, etag: r.etag ?? null, market };
  }

  async loadHistory(): Promise<History | null> {
    // Cached copy first (instant), then the conditional refresh; a refreshed
    // body wins. Both are Rust-side; the webview never fetches third-party.
    let best: History | null = null;
    try {
      const raw = await resolveInvoke()<string | null>('cached_history');
      if (raw) {
        const h = JSON.parse(raw) as History;
        if (isHistory(h)) best = h;
      }
    } catch {
      /* corrupt/absent cache → refresh decides */
    }
    try {
      const r = await resolveInvoke()<{ updated: boolean; body: string | null }>('refresh_history');
      if (r.updated && r.body) {
        const h = JSON.parse(r.body) as History;
        if (isHistory(h)) best = h;
      }
    } catch {
      /* IPC fault: keep whatever the cache gave us */
    }
    return best;
  }

  async submitPlan(items: PlanItemInput[]): Promise<PlanResponse> {
    try {
      return await resolveInvoke()<PlanResponse>('submit_plan', { items });
    } catch (e) {
      rethrowInvoke(e);
    }
  }
  async getPendingPlan(): Promise<PendingPlan | null> {
    // The command returns Option<PendingPlan> - null when there's nothing
    // queued, matching the HTTP path's 404 → null normalization.
    try {
      return await resolveInvoke()<PendingPlan | null>('get_pending_plan');
    } catch (e) {
      rethrowInvoke(e);
    }
  }
  async resumePendingPlan(): Promise<PlanResponse> {
    try {
      return await resolveInvoke()<PlanResponse>('resume_pending_plan');
    } catch (e) {
      rethrowInvoke(e);
    }
  }
  async discardPendingPlan(): Promise<unknown> {
    try {
      return await resolveInvoke()<null>('discard_pending_plan');
    } catch (e) {
      rethrowInvoke(e);
    }
  }
  async fetchOrders(): Promise<unknown> {
    try {
      return await resolveInvoke()<unknown>('fetch_orders');
    } catch (e) {
      rethrowInvoke(e);
    }
  }
  async updateOrder(orderId: string, patch: OrderPatch): Promise<unknown> {
    try {
      return await resolveInvoke()<unknown>('update_order', { orderId, patch });
    } catch (e) {
      rethrowInvoke(e);
    }
  }
  async deleteOrder(orderId: string): Promise<unknown> {
    try {
      return await resolveInvoke()<null>('delete_order', { orderId });
    } catch (e) {
      rethrowInvoke(e);
    }
  }
  async bulkVisibility(orderIds: string[], visible: boolean): Promise<{ results: ItemResult[] }> {
    try {
      const results = await resolveInvoke()<ItemResult[]>('bulk_visibility', { orderIds, visible });
      return { results };
    } catch (e) {
      rethrowInvoke(e);
    }
  }
}

// ---- Desktop-only WFM auth ops --------------------------------------------
// Not on the Transport interface: the hosted build has no login surface, so
// these are reachable only from desktop-gated UI. Secrets flow webview → Rust
// exactly once per call and are never returned.

export interface DesktopWfmStatus {
  /** An encrypted login envelope exists on disk. */
  logged_in: boolean;
  /** The desktop process holds the decrypted JWT in memory. */
  unlocked: boolean;
}

export async function desktopWfmStatus(): Promise<DesktopWfmStatus> {
  try {
    return await resolveInvoke()<DesktopWfmStatus>('wfm_auth_status');
  } catch (e) {
    rethrowInvoke(e);
  }
}

export async function desktopWfmLogout(): Promise<void> {
  try {
    await resolveInvoke()<null>('wfm_logout');
  } catch (e) {
    rethrowInvoke(e);
  }
}

export async function desktopWfmLogin(
  email: string,
  password: string,
  passphrase: string,
  platform: string,
  remember: boolean,
): Promise<void> {
  try {
    await resolveInvoke()<null>('wfm_login', { email, password, passphrase, platform, remember });
  } catch (e) {
    rethrowInvoke(e);
  }
}

export async function desktopWfmUnlock(passphrase: string, remember: boolean): Promise<void> {
  try {
    await resolveInvoke()<null>('unlock_jwt', { passphrase, remember });
  } catch (e) {
    rethrowInvoke(e);
  }
}

/**
 * Try the OS-keyring "remember on this device" key before showing the
 * passphrase modal. Never throws for a miss - false just means "ask the
 * human"; a genuine IPC fault still rethrows so the caller's fallback
 * (open the modal) runs.
 */
export async function desktopTrySilentUnlock(): Promise<boolean> {
  try {
    return await resolveInvoke()<boolean>('try_silent_unlock');
  } catch (e) {
    rethrowInvoke(e);
  }
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
}

/** Progress event the desktop emits per item during `desktopLiveTopPrices`. */
export const LIVE_TOP_PROGRESS_EVENT = 'live-top-progress';

/**
 * Exact-tier live asks/bids for up to 100 items, paced at WFM's 3 req/s
 * (≈17 s per 50) - listen on `LIVE_TOP_PROGRESS_EVENT` for `{done,total}`.
 */
export async function desktopLiveTopPrices(queries: LiveTopQuery[]): Promise<LiveTop[]> {
  try {
    return await resolveInvoke()<LiveTop[]>('live_top_prices', { queries });
  } catch (e) {
    rethrowInvoke(e);
  }
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

/**
 * The ≤20 cheapest matching auctions for one weapon's rivens, from WFM's v1
 * auctions search. The desktop paces calls through its shared 10/min auction
 * gate, so rapid "Show comps" clicks queue politely instead of tripping WFM.
 */
export async function desktopRivenComps(weapon: string): Promise<RivenAuction[]> {
  try {
    return await resolveInvoke()<RivenAuction[]>('riven_comps', { weapon });
  } catch (e) {
    rethrowInvoke(e);
  }
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

/** Rust emits this (a WatchOutcome) when a background pass notifies. */
export const WATCH_FIRED_EVENT = 'watch-fired';

export async function desktopListWatches(): Promise<Watch[]> {
  try { return await resolveInvoke()<Watch[]>('list_watches'); } catch (e) { rethrowInvoke(e); }
}
export async function desktopAddWatch(watch: NewWatch): Promise<Watch[]> {
  try { return await resolveInvoke()<Watch[]>('add_watch', { watch }); } catch (e) { rethrowInvoke(e); }
}
export async function desktopDeleteWatch(id: number): Promise<Watch[]> {
  try { return await resolveInvoke()<Watch[]>('delete_watch', { id }); } catch (e) { rethrowInvoke(e); }
}
export async function desktopCheckWatchesNow(): Promise<WatchOutcome[]> {
  try { return await resolveInvoke()<WatchOutcome[]>('check_watches_now'); } catch (e) { rethrowInvoke(e); }
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

/** Rust emits this when EE.log confirms a trade. */
export const TRADE_DETECTED_EVENT = 'trade-detected';
export interface TradeDetected {
  id: number;
  trade: { partner: string; kind: TradeRow['kind']; plat: number; items: TradeItem[]; log_stamp: string | null };
  /** [item name, new listing quantity (0 = deleted)] */
  adjusted: Array<[string, number]>;
}

export async function desktopListTrades(limit = 200): Promise<TradeRow[]> {
  try { return await resolveInvoke()<TradeRow[]>('list_trades', { limit }); } catch (e) { rethrowInvoke(e); }
}
export async function desktopEelogStatus(): Promise<EeLogStatus> {
  try { return await resolveInvoke()<EeLogStatus>('eelog_status'); } catch (e) { rethrowInvoke(e); }
}

/**
 * True inside the Tauri desktop webview. Keyed off `__TAURI_INTERNALS__` (the
 * runtime object Tauri v2 always injects), per the desktop spike - this is a
 * boot-time constant, not a per-call check.
 */
export function isDesktopRuntime(): boolean {
  return (
    typeof globalThis !== 'undefined' &&
    typeof (globalThis as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ !== 'undefined'
  );
}

/**
 * Boot-time transport selection. The desktop webview gets the real Tauri
 * transport; the hosted informational site gets HostedTransport (no interactive
 * ops).
 */
export function createTransport(): Transport {
  return isDesktopRuntime() ? new TauriTransport() : new HostedTransport();
}
