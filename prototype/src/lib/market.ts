// Fetches the pre-scraped market snapshot from /market.json. The snapshot
// is built server-side on a schedule (GitHub Actions cron) and committed
// to the repo, so the browser never has to call warframe.market directly.

import type { Market, MarketItemEntry } from './types';

const MARKET_URL = '/market.json';

let cached: Market | null = null;

export async function loadMarket(): Promise<Market> {
  if (cached) return cached;
  const r = await fetch(MARKET_URL);
  if (!r.ok) {
    throw new Error(
      `Couldn't load market snapshot (HTTP ${r.status}). ` +
        `In dev, run \`python3 scripts/csv_to_market_json.py\` to bootstrap one.`
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
