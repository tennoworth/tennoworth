
// Fetches the pre-scraped market snapshot from /market.json. The snapshot
// is built on the production box's schedule and served from the same origin,
// so the browser never has to call warframe.market directly.

import type { Market } from '../contracts/data';

const MARKET_URL = '/market.json';
export const MARKET_REFRESH_INTERVAL_MS = 30 * 60 * 1000;

let cached: Market | null = null;

export async function loadMarket(): Promise<Market> {
  if (cached) return cached;
  const r = await fetch(MARKET_URL);
  if (!r.ok) {
    throw new Error(
      `Couldn't load market snapshot (HTTP ${r.status}). ` +
        `In dev, run \`wfm-scrape build\` from the Rust workspace to bootstrap one.`
    );
  }
  cached = (await r.json()) as Market;
  return cached;
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
