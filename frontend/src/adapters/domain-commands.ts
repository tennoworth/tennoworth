import { resolveInvoke, rethrowInvoke } from './runtime';
import type { DomainRequest, DomainResponse, AdvisorRequest, HistoryRequest, SessionRequest } from '../contracts/generated/domain';
import type { SessionCandidate, selectSession } from '../domain/trade-session';

type Operation = DomainRequest['operation'];
type Input<K extends Operation> = Extract<DomainRequest, { operation: K }>['input'];
type Output<K extends Operation> = Extract<DomainResponse, { operation: K }>['result'];

export async function callDomain<K extends Operation>(operation: K, input: Input<K>): Promise<Output<K>> {
  try {
    const response = await resolveInvoke()<DomainResponse>('evaluate_domain', { request: { operation, input } });
    if (!response || response.operation !== operation) throw new Error('The calculation returned an unexpected response. Try again.');
    return response.result as Output<K>;
  } catch (error) { rethrowInvoke(error); }
}

export async function evaluateTradeSession(request: Omit<SessionRequest, 'candidates'> & { candidates: SessionCandidate[] }): Promise<ReturnType<typeof selectSession>> {
  const result = await callDomain('trade_session', { ...request, candidates: request.candidates.map(row => ({ ...row, subtype: row.subtype ?? undefined })) });
  const candidates = new Map(request.candidates.map(row => [row.key, row]));
  const rows = result.rows.map(row => {
    const source = candidates.get(row.key);
    if (!source) throw new Error('The trade suggestion contains an unknown item. Recalculate before listing.');
    return { ...row, market: source.market };
  });
  return { ...result, rows };
}

function object(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === 'object' && !Array.isArray(value);
}

export function evaluateAdvisor(request: AdvisorRequest) {
  const slugs = [...new Set(request.slugs)];
  const select = (items: unknown, project: (value: unknown) => unknown = value => value) => object(items)
    ? Object.fromEntries(slugs.filter(slug => Object.hasOwn(items, slug)).map(slug => [slug, project(items[slug])]))
    : items;
  // Advice only consumes these price/calendar fields and daily medians. Sending
  // the whole public archive copies thousands of unrelated item histories.
  const market = object(request.market) ? {
    items: select(request.market.items),
    calendar: request.market.calendar,
    set_to_parts: request.market.set_to_parts,
  } : request.market;
  const history = object(request.history) ? {
    start: request.history.start,
    items: select(request.history.items, series => object(series) ? { median: series.median } : series),
  } : request.history;
  return callDomain('advisor', { ...request, slugs, market, history });
}
export function evaluateHistory(request: HistoryRequest) { return callDomain('history', request); }
