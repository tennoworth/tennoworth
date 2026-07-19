// Client for the companion's DeepSeek-backed advisor endpoint. Peer of
// companion.ts — same X-Session-Token mechanism, same loopback-only origin.

import type { CompanionConfig } from './types';

// -------- Context building --------

/** The subset of ResultsTable/App.svelte's Row fields the advisor needs.
 *  Deliberately narrow (mirrors ListingReviewModal's InputRow pattern) so
 *  this module doesn't depend on the app's full Row shape. */
export interface AssistantSourceRow {
  name: string;
  owned: number;
  sellable: number;
  avg_price: number;
  volume_48h: number;
  sell_score: number;
  vault_status?: 'vaulted' | 'vaulting-soon' | 'available' | null;
}

export interface AssistantContextMeta {
  /** Pre-formatted "X ago" label for the market snapshot — the same string
   *  the app's own staleness indicator shows (App.svelte's `marketStaleness`).
   *  Passed through verbatim so this stays a pure function of its inputs
   *  instead of reading the wall clock itself. Null/absent → 'unknown'. */
  generatedAt?: string | null;
}

export interface AssistantContextTotals {
  distinct_items: number;
  total_owned: number;
  total_estimated_plat: number;
}

export interface AssistantContextItem {
  name: string;
  owned: number;
  sellable: number;
  avg_plat: number;
  vol_48h: number;
  vault: 'vaulted' | 'vaulting-soon' | 'available' | null;
}

export interface AssistantContext {
  market_data_age: string;
  totals: AssistantContextTotals;
  items: AssistantContextItem[];
}

const MAX_CONTEXT_ITEMS = 100;

/**
 * Builds the compact JSON `context` string sent to POST /assistant.
 *
 * Totals cover every row the caller passed (the user's whole sellable
 * inventory); the `items` list is capped to the top 100 by sell_score so
 * the payload stays small — the model doesn't need all 400 rows to answer
 * "what should I sell today", just the ones that matter. `vol_48h` is
 * deliberately named that (not "daily") — the underlying stat is a 48-hour
 * window, and calling it daily would be a real (if small) lie to the model.
 *
 * Returns null on empty/missing input so the caller can disable the chat
 * instead of sending an empty context.
 */
export function buildAssistantContext(
  rows: AssistantSourceRow[] | null | undefined,
  meta: AssistantContextMeta = {},
): string | null {
  if (!Array.isArray(rows) || rows.length === 0) return null;

  let total_owned = 0;
  let total_estimated_plat = 0;
  for (const r of rows) {
    total_owned += r.owned || 0;
    total_estimated_plat += (r.sellable || 0) * (r.avg_price || 0);
  }

  const items: AssistantContextItem[] = [...rows]
    .sort((a, b) => (b.sell_score || 0) - (a.sell_score || 0))
    .slice(0, MAX_CONTEXT_ITEMS)
    .map((r) => ({
      name: r.name,
      owned: r.owned,
      sellable: r.sellable,
      avg_plat: Math.round((r.avg_price || 0) * 10) / 10,
      vol_48h: r.volume_48h || 0,
      vault: r.vault_status ?? null,
    }));

  const context: AssistantContext = {
    market_data_age: meta?.generatedAt ?? 'unknown',
    totals: {
      distinct_items: rows.length,
      total_owned,
      total_estimated_plat: Math.round(total_estimated_plat),
    },
    items,
  };
  return JSON.stringify(context);
}

// -------- Asking --------

export interface AssistantMessage {
  role: 'user' | 'assistant';
  content: string;
}

export interface AssistantUsage {
  prompt_tokens: number;
  completion_tokens: number;
}

export interface AssistantAnswer {
  answer: string;
  usage: AssistantUsage;
}

export type AssistantErrorCode = 'no_api_key' | 'upstream' | 'auth' | 'network' | 'unknown';

/** Typed error askAssistant throws — the component maps `code` (and, for
 *  `upstream`, `detail`) to the user-facing copy the product spec fixes. */
export class AssistantError extends Error {
  code: AssistantErrorCode;
  detail?: string;
  constructor(code: AssistantErrorCode, detail?: string) {
    super(detail ? `${code}: ${detail}` : code);
    this.name = 'AssistantError';
    this.code = code;
    this.detail = detail;
  }
}

const MAX_HISTORY = 12;

const AUTH_REJECTED_MSG =
  'Token rejected — re-copy the full URL from the serve output (the token changes every time you restart serve).';

/** Maps an askAssistant() rejection to the exact copy the drawer shows. */
export function assistantErrorMessage(e: unknown): string {
  if (e instanceof AssistantError) {
    switch (e.code) {
      case 'no_api_key':
        return 'The companion has no DeepSeek API key configured — set DEEPSEEK_API_KEY or the deepseek-key config file.';
      case 'upstream':
        return `The AI service failed: ${e.detail ?? 'unknown error'}`;
      case 'auth':
        return AUTH_REJECTED_MSG; // same copy as App.svelte's companionError 401 handling
      case 'network':
        return 'Companion unreachable.';
      default:
        return e.detail || e.message || 'Something went wrong asking the advisor.';
    }
  }
  return e instanceof Error ? e.message : String(e);
}

/**
 * POST /assistant via the companion. `history` is trimmed to the last 12
 * messages here (in addition to whatever the caller already capped) so a
 * caller can pass a full transcript without thinking about the limit twice.
 * Throws AssistantError on any non-2xx or network failure; never resolves
 * with a companion error body.
 */
export async function askAssistant(
  config: CompanionConfig,
  question: string,
  history: AssistantMessage[],
  context: string | null,
): Promise<AssistantAnswer> {
  let r: Response;
  try {
    r = await fetch(`${config.baseUrl}/assistant`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json', 'X-Session-Token': config.token },
      body: JSON.stringify({
        question,
        history: (history || []).slice(-MAX_HISTORY),
        context: context ?? '',
      }),
    });
  } catch {
    throw new AssistantError('network');
  }

  let body: unknown = null;
  try { body = await r.json(); } catch { /* keep null */ }

  if (!r.ok) {
    if (r.status === 401) throw new AssistantError('auth');
    const errObj = body as { error?: unknown; detail?: unknown } | null;
    if (r.status === 503 && errObj?.error === 'no_api_key') throw new AssistantError('no_api_key');
    if (r.status === 502 && errObj?.error === 'upstream') {
      const detail = errObj?.detail;
      throw new AssistantError(
        'upstream',
        typeof detail === 'string' ? detail : JSON.stringify(detail ?? 'unknown error'),
      );
    }
    const fallback = typeof errObj?.error === 'string' ? errObj.error : `HTTP ${r.status}`;
    throw new AssistantError('unknown', fallback);
  }

  const resp = body as { answer?: unknown; usage?: Partial<AssistantUsage> } | null;
  if (!resp || typeof resp.answer !== 'string') {
    throw new AssistantError('unknown', 'Malformed response from companion.');
  }
  return {
    answer: resp.answer,
    usage: {
      prompt_tokens: resp.usage?.prompt_tokens ?? 0,
      completion_tokens: resp.usage?.completion_tokens ?? 0,
    },
  };
}
