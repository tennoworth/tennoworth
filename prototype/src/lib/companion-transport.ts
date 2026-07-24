// Shared fetch+parse core for talking to the companion's loopback HTTP
// server. companion.ts (plan/order routes) and assistant.ts (the DeepSeek
// relay) are otherwise-identical peers here — same X-Session-Token header,
// same Local Network Access opt-in (targetAddressSpace), same "parse JSON,
// don't throw if parsing fails" handling. Each caller keeps its own
// error-code -> exception mapping on top of the {ok, status, body} result.
export interface CompanionFetchResult {
  ok: boolean;
  status: number;
  body: unknown;
}

export async function fetchCompanionJson(
  baseUrl: string,
  token: string,
  path: string,
  opts: { method?: string; body?: unknown; signal?: AbortSignal } = {},
): Promise<CompanionFetchResult> {
  const headers: Record<string, string> = { 'X-Session-Token': token };
  const init: RequestInit = { method: opts.method ?? 'GET', headers, targetAddressSpace: 'loopback' };
  if (opts.signal) init.signal = opts.signal;
  if (opts.body !== undefined) {
    headers['Content-Type'] = 'application/json';
    init.body = JSON.stringify(opts.body);
  }
  const r = await fetch(`${baseUrl}${path}`, init);
  let body: unknown = null;
  try { body = await r.json(); } catch { /* keep null */ }
  return { ok: r.ok, status: r.status, body };
}
