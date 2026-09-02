// Fetches the pre-scraped market snapshot from /market.json. The snapshot
// is built on the production box's schedule and served from the same origin,
// so the browser never has to call warframe.market directly.

import type { Market, MarketItemEntry } from './types';

const MARKET_URL = '/market.json';
export const MARKET_REFRESH_INTERVAL_MS = 30 * 60 * 1000;

let cached: Market | null = null;

export async function loadMarket(): Promise<Market> {
  if (cached) return cached;
  const r = await fetch(MARKET_URL);
  if (!r.ok) {
    throw new Error(
      `Couldn't load market snapshot (HTTP ${r.status}). ` +
        `In dev, run \`wfm-scrape build\` (companion) to bootstrap one.`
    );
  }
  cached = (await r.json()) as Market;
  return cached;
}

export function lookup(market: Market | null | undefined, slug: string): MarketItemEntry | null {
  // Optional-chain guards a half-written `market.json` that's missing
  // the `items` key entirely (e.g. cron crashed mid-build). Without the
  // chain the resolver crashes on the first row and the page surfaces
  // an opaque error card instead of the "no market data" empty state.
  return market?.items?.[slug] ?? null;
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
