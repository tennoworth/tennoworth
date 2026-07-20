// Transport abstraction: the SPA's single seam between "talk to the loopback
// companion over HTTP" (hosted / `serve` build) and "call wfm-core directly
// over Tauri IPC" (desktop build). Selected ONCE at boot by sniffing the Tauri
// runtime — see `isDesktopRuntime()` / `createTransport()`.
//
// The high-level operation names mirror the existing companion.ts / assistant.ts
// functions so call sites read the same. HttpCompanionTransport delegates 1:1 to
// those modules (no behaviour change in browser mode — they keep their exports
// for the components that still import them directly). TauriTransport invokes a
// wfm-core-backed command per op; listing/order commands reject with a typed
// {code, message} CmdError which surfaces here as DesktopCmdError —
// `needs_login` / `needs_unlock` drive the SPA's login and passphrase dialogs
// (the desktop analogue of serve's 401 needs_login:true vs 503 split).

import type { CompanionConfig, PendingPlan, PlanResponse, ItemResult, Market } from './types';
import {
  pingCompanion,
  fetchInventory,
  submitPlan,
  getPendingPlan,
  resumePendingPlan,
  discardPendingPlan,
  fetchOrders,
  updateOrder,
  deleteOrder,
  bulkVisibility,
  type PingResponse,
  type PlanItemInput,
  type OrderPatch,
} from './companion';
import {
  askAssistant,
  AssistantError,
  type AssistantMessage,
  type AssistantAnswer,
} from './assistant';

export type { PingResponse, PlanItemInput, OrderPatch } from './companion';
export type { AssistantMessage, AssistantAnswer } from './assistant';

/**
 * A desktop command rejection, rehydrated from the Rust CmdError
 * `{ code, message }` the invoke promise rejects with. Callers branch on
 * `code` (`needs_login` / `needs_unlock` open the auth dialogs;
 * `bad_passphrase` stays in the passphrase dialog; everything else shows
 * `message` verbatim). Never carries the JWT, passphrase, or password —
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
 * / offline / error it is false with no `market` — the caller keeps what it has.
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
 * The operations the app performs against the companion / core, config-free.
 * The HTTP implementation binds the loopback URL+token internally (via a
 * getter); the Tauri implementation needs no config at all — that whole surface
 * is stripped from the desktop build.
 */
export interface Transport {
  readonly mode: 'http' | 'tauri';
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
   * the box — it must make NO third-party fetch). Never rejects on network
   * failure; a failed refresh returns `{ updated: false }`.
   */
  refreshMarket(): Promise<MarketRefreshResult>;
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
  askAssistant(
    question: string,
    history: AssistantMessage[],
    context: string | null,
  ): Promise<AssistantAnswer>;
}

/**
 * HTTP transport: today's loopback companion. A thin wrapper over companion.ts
 * + assistant.ts that supplies the current CompanionConfig from a getter (the
 * config changes at runtime as the user connects / disconnects, so we read it
 * lazily rather than capture a snapshot). Every method preserves the wrapped
 * module's semantics verbatim — including CompanionUnreachableError from
 * health() and the `targetAddressSpace: 'loopback'` LNA options.
 */
export class HttpCompanionTransport implements Transport {
  readonly mode = 'http' as const;
  #getConfig: () => CompanionConfig | null;

  constructor(getConfig: () => CompanionConfig | null) {
    this.#getConfig = getConfig;
  }

  // Loopback ops are only ever dispatched once the UI has a verified
  // connection, so a null here is a programming error, not a user state.
  #cfg(): CompanionConfig {
    const c = this.#getConfig();
    if (!c) throw new Error('Not connected to the companion.');
    return c;
  }

  health(timeoutMs?: number): Promise<PingResponse> {
    return timeoutMs === undefined
      ? pingCompanion(this.#cfg())
      : pingCompanion(this.#cfg(), timeoutMs);
  }
  // The hosted build has no client-side market cache and refreshes nothing: the
  // box serves a fresh /market.json same-origin. Both are pure no-ops and make
  // NO third-party fetch (prototype/CLAUDE.md's cardinal rule) — market.ts's
  // existing same-origin fetch stays the sole market load in browser mode.
  async loadCachedMarket(): Promise<Market | null> {
    return null;
  }
  async refreshMarket(): Promise<MarketRefreshResult> {
    return { updated: false, updatedAt: null, etag: null };
  }
  fetchInventory(): Promise<unknown> {
    return fetchInventory(this.#cfg());
  }
  submitPlan(items: PlanItemInput[]): Promise<PlanResponse> {
    return submitPlan(this.#cfg(), items);
  }
  getPendingPlan(): Promise<PendingPlan | null> {
    return getPendingPlan(this.#cfg());
  }
  resumePendingPlan(): Promise<PlanResponse> {
    return resumePendingPlan(this.#cfg());
  }
  discardPendingPlan(): Promise<unknown> {
    return discardPendingPlan(this.#cfg());
  }
  fetchOrders(): Promise<unknown> {
    return fetchOrders(this.#cfg());
  }
  updateOrder(orderId: string, patch: OrderPatch): Promise<unknown> {
    return updateOrder(this.#cfg(), orderId, patch);
  }
  deleteOrder(orderId: string): Promise<unknown> {
    return deleteOrder(this.#cfg(), orderId);
  }
  bulkVisibility(orderIds: string[], visible: boolean): Promise<{ results: ItemResult[] }> {
    return bulkVisibility(this.#cfg(), orderIds, visible);
  }
  askAssistant(
    question: string,
    history: AssistantMessage[],
    context: string | null,
  ): Promise<AssistantAnswer> {
    return askAssistant(this.#cfg(), question, history, context);
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

/**
 * Tauri transport: each op is a wfm-core-backed command. The listing/order ops
 * mirror serve's HTTP routes 1:1 (submit_plan ↔ POST /plan, get_pending_plan ↔
 * GET /plan/pending, …); their rejections surface as DesktopCmdError so the
 * caller can branch on `needs_login` / `needs_unlock`.
 */
export class TauriTransport implements Transport {
  readonly mode = 'tauri' as const;

  async health(): Promise<PingResponse> {
    return await resolveInvoke()<PingResponse>('health');
  }

  async fetchInventory(): Promise<unknown> {
    // The command returns the inventory JSON as a string (the exact bytes the
    // CLI would write); a rejected invoke carries wfm-core's graceful message
    // (e.g. "Warframe doesn't appear to be running…").
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

  async submitPlan(items: PlanItemInput[]): Promise<PlanResponse> {
    try {
      return await resolveInvoke()<PlanResponse>('submit_plan', { items });
    } catch (e) {
      rethrowInvoke(e);
    }
  }
  async getPendingPlan(): Promise<PendingPlan | null> {
    // The command returns Option<PendingPlan> — null when there's nothing
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
  async askAssistant(
    question: string,
    history: AssistantMessage[],
    context: string | null,
  ): Promise<AssistantAnswer> {
    // Map CmdError codes onto the AssistantError contract AssistantChat
    // already renders, so the drawer copy is identical in both modes.
    try {
      return await resolveInvoke()<AssistantAnswer>('ask_assistant', { question, history, context });
    } catch (e) {
      const o = e as { code?: unknown; message?: unknown } | null;
      if (o && typeof o.code === 'string') {
        const detail = typeof o.message === 'string' ? o.message : undefined;
        if (o.code === 'no_api_key') throw new AssistantError('no_api_key');
        if (o.code === 'upstream') throw new AssistantError('upstream', detail);
        throw new AssistantError('unknown', detail ?? o.code);
      }
      rethrowInvoke(e);
    }
  }
}

// ---- Desktop-only WFM auth ops --------------------------------------------
// Not on the Transport interface: the browser build has no login surface (serve
// prompts in its own terminal), so these are reachable only from desktop-gated
// UI. Secrets flow webview → Rust exactly once per call and are never returned.

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

export async function desktopWfmLogin(
  email: string,
  password: string,
  passphrase: string,
  platform: string,
): Promise<void> {
  try {
    await resolveInvoke()<null>('wfm_login', { email, password, passphrase, platform });
  } catch (e) {
    rethrowInvoke(e);
  }
}

export async function desktopWfmUnlock(passphrase: string): Promise<void> {
  try {
    await resolveInvoke()<null>('unlock_jwt', { passphrase });
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

/**
 * True inside the Tauri desktop webview. Keyed off `__TAURI_INTERNALS__` (the
 * runtime object Tauri v2 always injects), per the desktop spike — this is a
 * boot-time constant, not a per-call check.
 */
export function isDesktopRuntime(): boolean {
  return (
    typeof globalThis !== 'undefined' &&
    typeof (globalThis as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ !== 'undefined'
  );
}

/**
 * Boot-time transport selection. Pass a getter for the current CompanionConfig
 * — it's only ever read by the HTTP transport; the Tauri transport ignores it.
 */
export function createTransport(getConfig: () => CompanionConfig | null): Transport {
  return isDesktopRuntime() ? new TauriTransport() : new HttpCompanionTransport(getConfig);
}
