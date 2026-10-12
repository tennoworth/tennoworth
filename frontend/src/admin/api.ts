import type { Change, DashboardState } from '../contracts/generated/publisher';
import { parseAnnouncements } from '../domain/announcements';

export interface Publisher {
  load(): Promise<DashboardState>;
  change(change: Change): Promise<DashboardState>;
}

async function request(change?: Change): Promise<DashboardState> {
  let response: Response;
  try {
    response = await fetch(change ? '/api/announcements/change' : '/api/announcements', {
      method: change ? 'POST' : 'GET', cache: 'no-store', credentials: 'same-origin', redirect: 'manual',
      headers: { Accept: 'application/json', ...(change ? { 'Content-Type': 'application/json' } : {}) },
      body: change ? JSON.stringify(change) : undefined, signal: AbortSignal.timeout(10_000),
    });
  } catch {
    throw new Error('The publisher could not be reached. Refresh to check whether your request was saved.');
  }
  // An expired Access session never reaches the publisher: the edge redirects to its login page.
  if (response.type === 'opaqueredirect' || response.status === 401) {
    throw new Error('Your sign-in has expired. Unsaved edits stay in this tab: reload the page to sign in again, then retry.');
  }
  const value = await response.json().catch(() => null);
  if (!response.ok) throw new Error(value?.message || 'The publisher could not be reached. Refresh to check whether your request was saved.');
  if (!value || typeof value.actor !== 'string' || !value.snapshot
    || !Number.isSafeInteger(value.snapshot.revision) || !Array.isArray(value.snapshot.messages)
    || !Array.isArray(value.snapshot.history) || !parseAnnouncements(value.snapshot.feed)) {
    throw new Error('The publisher returned an unreadable response. Your edits remain in the form.');
  }
  return value as DashboardState;
}

export const publisher: Publisher = { load: () => request(), change: request };
