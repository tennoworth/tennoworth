
// Fetches the pre-scraped market snapshot from /market.json. The snapshot
// is built on the production box's schedule and served from the same origin,
// so the browser never has to call warframe.market directly.

import type { Market } from '../contracts/data';
import { isMarket, replacesMarket } from '../domain/market';

const MARKET_URL = '/market.json';
export const MARKET_REFRESH_INTERVAL_MS = 30 * 60 * 1000;

let cached: Market | null = null;

async function fetchMarket(response: Promise<Response>): Promise<Market> {
  const r = await response;
  if (!r.ok) {
    throw new Error(
      `Couldn't load market snapshot (HTTP ${r.status}). ` +
        `In dev, run \`wfm-scrape build\` from the Rust workspace to bootstrap one.`
    );
  }
  const body: unknown = await r.json();
  // A 200 is not a snapshot: `null` used to leave the page loading forever.
  if (!isMarket(body)) throw new Error("The market snapshot is incomplete or malformed.");
  return body;
}

export async function loadMarket(): Promise<Market> {
  if (cached) return cached;
  cached = await fetchMarket(fetch(MARKET_URL));
  return cached;
}

/** Ask the server whether a newer snapshot exists. Resolves to it only when it
 *  is valid and replaces `current` under the shared ordering rule; null means
 *  keep what is shown.
 *  Rejects on a failed request, which also keeps what is shown. */
export async function revalidateMarket(current: Market): Promise<Market | null> {
  // `no-cache` revalidates with the server (a 304 is cheap) instead of
  // trusting the browser's copy for its whole max-age.
  const next = await fetchMarket(fetch(MARKET_URL, { cache: 'no-cache' }));
  if (!replacesMarket(current.updated_at, next.updated_at, Date.now())) return null;
  cached = next;
  return next;
}

/** A returning visitor gets a check at most this often; the snapshot itself
 *  is rebuilt about every two hours. */
export const HOSTED_RETURN_MIN_INTERVAL_MS = 10 * 60 * 1000;

/**
 * Keep an open site tab's snapshot current: every 30 minutes and on reconnect
 * while the tab is visible, and when the visitor returns to it if the last
 * check is ten minutes old. Returns a stop function.
 */
export function startHostedMarketRevalidation(
  revalidate: () => Promise<void>,
  runtime: Window = window,
): () => void {
  const doc = runtime.document;
  let lastCheck = Date.now();
  const loop = startMarketRefreshLoop(async () => {
    if (doc.hidden) return;
    lastCheck = Date.now();
    await revalidate();
  }, runtime);
  const onReturn = (): void => {
    if (!doc.hidden && Date.now() - lastCheck >= HOSTED_RETURN_MIN_INTERVAL_MS) loop.trigger();
  };
  doc.addEventListener('visibilitychange', onReturn);
  runtime.addEventListener('focus', onReturn);
  return () => {
    loop.stop();
    doc.removeEventListener('visibilitychange', onReturn);
    runtime.removeEventListener('focus', onReturn);
  };
}

export interface MarketRefreshLoop {
  trigger(): void;
  stop(): void;
}

/**
 * Retry a desktop market refresh on reconnect and periodically while the app
 * remains open. Concurrent triggers collapse into one queued follow-up so an
 * online event arriving during a slow failed request is not lost.
 */
export function startMarketRefreshLoop(
  refresh: () => Promise<void>,
  runtime: Window = window,
): MarketRefreshLoop {
  let stopped = false;
  let running = false;
  let runAgain = false;

  const trigger = (): void => {
    if (stopped) return;
    if (running) {
      runAgain = true;
      return;
    }
    running = true;
    void refresh()
      .catch(() => {})
      .finally(() => {
        running = false;
        if (runAgain && !stopped) {
          runAgain = false;
          trigger();
        }
      });
  };

  const onOnline = (): void => trigger();
  runtime.addEventListener('online', onOnline);
  const timer = runtime.setInterval(trigger, MARKET_REFRESH_INTERVAL_MS);

  return {
    trigger,
    stop(): void {
      if (stopped) return;
      stopped = true;
      runAgain = false;
      runtime.clearInterval(timer);
      runtime.removeEventListener('online', onOnline);
    },
  };
}
