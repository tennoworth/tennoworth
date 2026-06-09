// Client for the companion's loopback HTTP server. Persists the
// URL+token in localStorage so the user pastes it once.

import type { CompanionConfig, PendingPlan, PlanResponse, ItemResult } from './types';

const STORAGE_KEY = 'wfminv:companion-v1';

export function loadCompanionConfig(): CompanionConfig | null {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    return raw ? (JSON.parse(raw) as CompanionConfig) : null;
  } catch {
    return null;
  }
}

export function saveCompanionConfig(cfg: CompanionConfig): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(cfg));
  } catch {
    /* ignore */
  }
}

export function clearCompanionConfig(): void {
  try { localStorage.removeItem(STORAGE_KEY); } catch { /* ignore */ }
}

// Loopback hosts the companion ever binds to. Anything else pasted into
// the connect box (a public URL, an internal LAN IP, a phishing host) is
// rejected so we never persist a token destined for a non-loopback origin
// and never send the session token off-machine.
const LOOPBACK_HOSTS = new Set(['127.0.0.1', 'localhost', '[::1]', '::1']);

/**
 * Parses the URL the companion prints at startup, e.g.
 *   http://127.0.0.1:45891?token=abc123
 * Returns { baseUrl: 'http://127.0.0.1:45891', token: 'abc123' }.
 *
 * Hard-rejects anything that isn't `http://` to a loopback host — the
 * companion only ever binds 127.0.0.1, so a non-loopback URL is either a
 * paste mistake or a phishing attempt.
 */
export function parseCompanionUrl(input: string): CompanionConfig {
  let u: URL;
  try {
    u = new URL(input.trim());
  } catch {
    throw new Error('Not a valid URL.');
  }
  if (u.protocol !== 'http:') {
    throw new Error(
      `Companion URL must be http:// — got ${u.protocol}. The companion only ever binds 127.0.0.1, so an https URL is almost certainly a paste of the wrong line.`
    );
  }
  if (!LOOPBACK_HOSTS.has(u.hostname)) {
    throw new Error(
      `Companion URL host must be 127.0.0.1 or localhost — got ${u.hostname}. Refusing to send your session token off-machine.`
    );
  }
  const token = u.searchParams.get('token');
  if (!token) throw new Error('URL is missing ?token=… — re-paste the full line.');
  return {
    baseUrl: `${u.protocol}//${u.host}`,
    token,
  };
}

export interface PingResponse {
  ok: boolean;
  platform?: string;
}

export async function pingCompanion(cfg: CompanionConfig): Promise<PingResponse> {
  const r = await fetch(`${cfg.baseUrl}/health`);
  if (!r.ok) throw new Error(`Health check failed: HTTP ${r.status}`);
  return r.json() as Promise<PingResponse>;
}

/** Plan items submitted to the companion via POST /plan. */
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

/**
 * POST a plan to the companion. Resolves with the {plan_id, results[]} response.
 * Throws on network errors or non-2xx HTTP.
 */
export async function submitPlan(cfg: CompanionConfig, items: PlanItemInput[]): Promise<PlanResponse> {
  return (await callCompanion(cfg, 'POST', '/plan', { items })) as PlanResponse;
}

/** Bulk-toggle the visibility of a list of order_ids. Returns a per-order result list. */
export async function bulkVisibility(
  cfg: CompanionConfig,
  orderIds: string[],
  visible: boolean,
): Promise<{ results: ItemResult[] }> {
  return (await callCompanion(cfg, 'POST', '/orders/visibility', {
    order_ids: orderIds,
    visible,
  })) as { results: ItemResult[] };
}

/** Fetch the user's current WFM orders via the companion. */
export async function fetchOrders(cfg: CompanionConfig): Promise<unknown> {
  return await callCompanion(cfg, 'GET', '/orders');
}

export interface OrderPatch {
  platinum?: number;
  quantity?: number;
  visible?: boolean;
  rank?: number;
}

/** PATCH a single order: price / quantity / visible / rank. */
export async function updateOrder(
  cfg: CompanionConfig,
  orderId: string,
  patch: OrderPatch,
): Promise<unknown> {
  return await callCompanion(cfg, 'PATCH', `/order/${orderId}`, patch);
}

/** DELETE a single order. */
export async function deleteOrder(cfg: CompanionConfig, orderId: string): Promise<unknown> {
  return await callCompanion(cfg, 'DELETE', `/order/${orderId}`);
}

/**
 * GET the pending plan if the companion has one queued from an interrupted
 * batch. Returns null when there isn't one (companion 404).
 */
export async function getPendingPlan(cfg: CompanionConfig): Promise<PendingPlan | null> {
  try {
    return (await callCompanion(cfg, 'GET', '/plan/pending')) as PendingPlan;
  } catch (e) {
    const msg = e instanceof Error ? e.message : String(e);
    if (/no pending plan|HTTP 404/i.test(msg)) return null;
    throw e;
  }
}

/** POST /plan/resume — re-runs the pending plan, skipping completed items. */
export async function resumePendingPlan(cfg: CompanionConfig): Promise<PlanResponse> {
  return (await callCompanion(cfg, 'POST', '/plan/resume')) as PlanResponse;
}

/** DELETE /plan/pending — discard the on-disk pending plan. */
export async function discardPendingPlan(cfg: CompanionConfig): Promise<unknown> {
  return await callCompanion(cfg, 'DELETE', '/plan/pending');
}

async function callCompanion(
  cfg: CompanionConfig,
  method: string,
  path: string,
  body?: unknown,
): Promise<unknown> {
  const headers: Record<string, string> = { 'X-Session-Token': cfg.token };
  const init: RequestInit = { method, headers };
  if (body !== undefined) {
    headers['Content-Type'] = 'application/json';
    init.body = JSON.stringify(body);
  }
  const r = await fetch(`${cfg.baseUrl}${path}`, init);
  let resp: unknown = null;
  try { resp = await r.json(); } catch { /* keep null */ }
  if (!r.ok) {
    const respObj = resp as { error?: unknown } | null;
    const msg = respObj?.error ?? `HTTP ${r.status}`;
    throw new Error(typeof msg === 'string' ? msg : JSON.stringify(msg));
  }
  return resp;
}
