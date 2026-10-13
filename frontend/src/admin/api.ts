import type { Market } from '../contracts/data';
import type { Change, DashboardState, EventChange, PublicFeedCheck, RunJob, StatusState } from '../contracts/generated/publisher';
import { parseAnnouncements } from '../domain/announcements';

export interface Publisher {
  load(): Promise<DashboardState>;
  change(change: Change): Promise<DashboardState>;
  changeEvent(change: EventChange): Promise<DashboardState>;
  publicFeed(): Promise<PublicFeedCheck>;
  /** The live market snapshot, for its calendar dates. */
  market(): Promise<Market>;
}

export interface BoxApi {
  load(): Promise<StatusState>;
  run(job: string): Promise<StatusState>;
}

const unreachable = 'The publisher could not be reached. Refresh to check whether your request was saved.';

async function call(url: string, body?: unknown): Promise<unknown> {
  let response: Response;
  try {
    response = await fetch(url, {
      method: body ? 'POST' : 'GET', cache: 'no-store', credentials: 'same-origin', redirect: 'manual',
      headers: { Accept: 'application/json', ...(body ? { 'Content-Type': 'application/json' } : {}) },
      body: body ? JSON.stringify(body) : undefined, signal: AbortSignal.timeout(10_000),
    });
  } catch {
    throw new Error(unreachable);
  }
  // An expired Access session never reaches the publisher: the edge redirects to its login page.
  if (response.type === 'opaqueredirect' || response.status === 401) {
    throw new Error('Your sign-in has expired. Unsaved edits stay in this tab: reload the page to sign in again, then retry.');
  }
  const value = await response.json().catch(() => null);
  if (!response.ok) throw new Error(value?.message || unreachable);
  return value;
}

async function request(change?: Change, url = change ? '/api/announcements/change' : '/api/announcements', body: unknown = change): Promise<DashboardState> {
  const value = await call(url, body) as DashboardState | null;
  if (!value || typeof value.actor !== 'string' || !value.snapshot
    || !Number.isSafeInteger(value.snapshot.revision) || !Array.isArray(value.snapshot.messages)
    || !Array.isArray(value.snapshot.history) || !parseAnnouncements(value.snapshot.feed)) {
    throw new Error('The publisher returned an unreadable response. Your edits remain in the form.');
  }
  return value;
}

async function status(job?: string): Promise<StatusState> {
  const value = await call(job ? '/api/jobs/run' : '/api/status', job ? { job } satisfies RunJob : undefined) as StatusState | null;
  const box = value?.status;
  if (!value || typeof value.actor !== 'string' || !box || typeof box.collected_at !== 'string'
    || !Array.isArray(box.services) || !Array.isArray(box.jobs) || !Array.isArray(box.alerts)
    || !Array.isArray(box.problems) || !Array.isArray(box.failed_units) || !box.backup) {
    throw new Error('The publisher returned an unreadable status report.');
  }
  return value;
}

async function publicFeed(): Promise<PublicFeedCheck> {
  const value = await call('/api/announcements/public') as PublicFeedCheck | null;
  if (!value || typeof value.checked_at !== 'string' || typeof value.state !== 'string') throw new Error('The publisher returned an unreadable feed check.');
  return value;
}

async function market(): Promise<Market> {
  let response: Response;
  try { response = await fetch('/market.json', { cache: 'no-store', credentials: 'same-origin', redirect: 'manual', signal: AbortSignal.timeout(20_000) }); }
  catch { throw new Error('The market snapshot could not be reached.'); }
  if (response.type === 'opaqueredirect') throw new Error('Your sign-in has expired. Reload the page to sign in again.');
  const value = response.ok ? await response.json().catch(() => null) : null;
  if (!value || typeof value !== 'object') throw new Error(`The market snapshot is unavailable${response.ok ? '' : ` (HTTP ${response.status})`}.`);
  return value as Market;
}

export const publisher: Publisher = { load: () => request(), change: request, changeEvent: (change) => request(undefined, '/api/announcements/events', change), publicFeed, market };
export const boxApi: BoxApi = { load: () => status(), run: status };
