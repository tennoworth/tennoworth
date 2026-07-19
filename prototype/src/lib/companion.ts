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
  // The website itself runs on a loopback port in dev (5173); pasting the
  // browser address bar passes every check above, then fails opaquely against
  // SPA HTML. The companion prints a RANDOM port — catch the website port here.
  const webHost = typeof location !== 'undefined' ? location.host : '';
  if (u.host === webHost || u.port === '5173' || u.port === '4173') {
    throw new Error(
      `That looks like this website's address (port ${u.port || '80'}), not the companion. Run \`wfm-fetch-inventory serve\` and paste the http://127.0.0.1:<random-port>?token=… line it prints.`
    );
  }
  const token = u.searchParams.get('token');
  if (!token) throw new Error('URL is missing ?token=… — re-paste the full line the companion printed.');
  return {
    baseUrl: `${u.protocol}//${u.host}`,
    token,
  };
}

export interface PingResponse {
  ok: boolean;
  platform?: string;
}

/**
 * Thrown ONLY when the `/health` fetch() itself rejects (a network-level
 * TypeError) — connection refused because `serve` isn't running, or the browser
 * blocking a loopback request from an HTTPS origin (Chromium's Local Network
 * Access gate). It is deliberately NOT thrown for a non-OK HTTP response: a 500
 * or an HTML answer means we DID reach something, which needs different
 * guidance. Callers key off this type to distinguish "couldn't connect at all"
 * from "connected but wrong".
 */
export class CompanionUnreachableError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'CompanionUnreachableError';
  }
}

// Firefox 147+ under ETP Strict prompts for Local Network Access and, while
// that prompt is undecided, a fetch to 127.0.0.1 HANGS indefinitely — it
// neither resolves nor rejects (Chromium can wedge similarly). Without a
// timeout the connect flow spins forever and never reaches a failure state, so
// the "unreachable" banner never appears. Timing the probe out turns the hang
// into a rejection we can classify. Only /health and the connect-time
// /plan/pending probe get a timeout; the listing/order routes deliberately do
// NOT — the first listing call legitimately blocks while serve prompts for a
// passphrase in its own terminal.
const HEALTH_TIMEOUT_MS = 8000;
const PENDING_PLAN_TIMEOUT_MS = 10000;

export async function pingCompanion(
  cfg: CompanionConfig,
  timeoutMs: number = HEALTH_TIMEOUT_MS,
): Promise<PingResponse> {
  let r: Response;
  try {
    r = await fetch(`${cfg.baseUrl}/health`, { signal: AbortSignal.timeout(timeoutMs) });
  } catch {
    // Every rejection here — connection refused (serve down), an HTTPS origin
    // blocking loopback, or the timeout firing on a hung permission prompt — is
    // "we never reached the companion" from the user's side, so all classify as
    // unreachable. Non-OK responses and bad JSON below stay plain Errors: those
    // mean we DID reach something.
    throw new CompanionUnreachableError(`Couldn't reach the companion at ${cfg.baseUrl}. Is \`serve\` still running in a terminal?`);
  }
  if (!r.ok) {
    throw new Error(`Health check failed: HTTP ${r.status}. Check the URL is the companion's (random port), not the website's (5173).`);
  }
  let body: unknown;
  try {
    body = await r.json();
  } catch {
    // A 200 that isn't JSON means we hit a web server, not the companion —
    // classically the website's own port answering with index.html.
    throw new Error(`That URL answered with a web page, not the companion. Paste the http://127.0.0.1:<random-port>?token=… line that \`serve\` printed — not the website.`);
  }
  const resp = body as PingResponse;
  if (!resp || resp.ok !== true) {
    throw new Error(`Unexpected response from ${cfg.baseUrl}/health — is that really the companion?`);
  }
  return resp;
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

/**
 * Pull inventory.json straight from the companion (it memory-scans the running
 * game on demand) — no file, no drag-in. Returns the parsed inventory object.
 * Throws with the companion's actionable message (e.g. game not running) on 503.
 */
export async function fetchInventory(cfg: CompanionConfig): Promise<unknown> {
  return await callCompanion(cfg, 'GET', '/inventory');
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
    return (await callCompanion(
      cfg, 'GET', '/plan/pending', undefined, AbortSignal.timeout(PENDING_PLAN_TIMEOUT_MS),
    )) as PendingPlan;
  } catch (e) {
    // A timeout here is NOT "unreachable" — /health already proved the companion
    // answers, and only the /health fetch may classify as unreachable. But we
    // must not swallow it as "no pending plan" either, or we'd hide an
    // interrupted batch — surface it as a plain Error.
    if (e instanceof DOMException && (e.name === 'TimeoutError' || e.name === 'AbortError')) {
      throw new Error(`The companion didn't answer /plan/pending in time — is \`serve\` still responding?`);
    }
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
  signal?: AbortSignal,
): Promise<unknown> {
  const headers: Record<string, string> = { 'X-Session-Token': cfg.token };
  const init: RequestInit = { method, headers };
  if (signal) init.signal = signal;
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
