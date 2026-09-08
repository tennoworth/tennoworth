import type { Market, MarketItemEntry } from '../contracts/data';

const SURFACE_STALE_MS = 3 * 24 * 60 * 60 * 1000;


export function lookup(market: Market | null | undefined, slug: string): MarketItemEntry | null {
  // Optional-chain guards a half-written `market.json` that's missing
  // the `items` key entirely (e.g. cron crashed mid-build). Without the
  // chain the resolver crashes on the first row and the page surfaces
  // an opaque error card instead of the "no market data" empty state.
  return market?.items?.[slug] ?? null;
}


export function staleSurfaceTimestamp(
  market: Market | null | undefined,
  key: string,
  now: number,
): string | null {
  const provenance = market?.surface_provenance?.[key];
  // The old payload is byte-for-byte current when its upstream content hash
  // was just revalidated; its original download time remains audit metadata.
  if (provenance?.disposition === 'preserved_unchanged') return null;

  const provenanceStamp = provenance?.data_fetched_at;
  const stamp = provenanceStamp && Number.isFinite(Date.parse(provenanceStamp))
    ? provenanceStamp
    : market?.surface_fetched_at?.[key];
  if (!stamp) return null;
  const stampMs = Date.parse(stamp);
  if (!Number.isFinite(stampMs) || now - stampMs < SURFACE_STALE_MS) return null;
  return stamp;
}
