// Transport abstraction: the SPA's single seam between "talk to the loopback
// companion over HTTP" (hosted / `serve` build) and "call wfm-core directly
// over Tauri IPC" (desktop build). Selected ONCE at boot by sniffing the Tauri
// runtime — see `isDesktopRuntime()` / `createTransport()`.
//
// The high-level operation names mirror the existing companion.ts / assistant.ts
// functions so call sites read the same. HttpCompanionTransport delegates 1:1 to
// those modules (no behaviour change in browser mode — they keep their exports
// for the components that still import them directly). TauriTransport invokes a
// wfm-core-backed command per implemented op; the ops that still need the
// passphrase/login UI (listing, orders, assistant) are declared but throw
// NotImplementedError until that surface lands (next chunk), and the desktop UI
// hides those affordances.

import type { CompanionConfig, PendingPlan, PlanResponse, ItemResult } from './types';
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
import { askAssistant, type AssistantMessage, type AssistantAnswer } from './assistant';

export type { PingResponse, PlanItemInput, OrderPatch } from './companion';
export type { AssistantMessage, AssistantAnswer } from './assistant';

/**
 * Thrown by TauriTransport for the operations that are declared on the
 * interface but not wired in the desktop build yet (listing / orders /
 * assistant — they need the passphrase UI). `op` names the operation so the
 * caller can log or branch; the desktop UI hides these affordances so a user
 * never reaches one.
 */
export class NotImplementedError extends Error {
  op: string;
  constructor(op: string) {
    super(`${op} is not available in the desktop app yet.`);
    this.name = 'NotImplementedError';
    this.op = op;
  }
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
 * Tauri transport: each op is a wfm-core-backed command. Only the two the
 * first desktop flow needs are wired — `health` and `fetchInventory`
 * (→ scan_inventory). The listing / order / assistant ops throw
 * NotImplementedError until the desktop passphrase surface lands.
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

  submitPlan(): Promise<PlanResponse> {
    throw new NotImplementedError('submitPlan');
  }
  getPendingPlan(): Promise<PendingPlan | null> {
    throw new NotImplementedError('getPendingPlan');
  }
  resumePendingPlan(): Promise<PlanResponse> {
    throw new NotImplementedError('resumePendingPlan');
  }
  discardPendingPlan(): Promise<unknown> {
    throw new NotImplementedError('discardPendingPlan');
  }
  fetchOrders(): Promise<unknown> {
    throw new NotImplementedError('fetchOrders');
  }
  updateOrder(): Promise<unknown> {
    throw new NotImplementedError('updateOrder');
  }
  deleteOrder(): Promise<unknown> {
    throw new NotImplementedError('deleteOrder');
  }
  bulkVisibility(): Promise<{ results: ItemResult[] }> {
    throw new NotImplementedError('bulkVisibility');
  }
  askAssistant(): Promise<AssistantAnswer> {
    throw new NotImplementedError('askAssistant');
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
