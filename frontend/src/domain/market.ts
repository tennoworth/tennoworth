import type { Market, MarketItemEntry } from '../contracts/data';

const SURFACE_STALE_MS = 3 * 24 * 60 * 60 * 1000;

export function surfaceHasEvidence(disposition: string | undefined): boolean {
  return disposition === undefined || [
    'published_fresh', 'merged_partial', 'cleared_authoritative_empty', 'preserved_unchanged',
  ].includes(disposition);
}

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
  if (provenance?.disposition && !surfaceHasEvidence(provenance.disposition)
    && provenance.disposition !== 'preserved_invalid'
    && provenance.disposition !== 'preserved_unavailable') {
    return 'unknown';
  }
  // The old payload is byte-for-byte current when its upstream content hash
  // was just revalidated; its original download time remains audit metadata.
  if (provenance?.disposition === 'preserved_unchanged') return null;

  const provenanceStamp = provenance?.data_fetched_at;
  const stamp = provenanceStamp === ''
    ? ''
    : provenanceStamp && Number.isFinite(Date.parse(provenanceStamp))
      ? provenanceStamp
      : market?.surface_fetched_at?.[key];
  if (!stamp) return provenance || (market && key in market) ? 'unknown' : null;
  const stampMs = Date.parse(stamp);
  if (!Number.isFinite(stampMs)) return 'unknown';
  if (now - stampMs < SURFACE_STALE_MS) return null;
  return stamp;
}

// Native refresh uses the same bound; tests/fixtures/market-contract pins both.
const MAX_FUTURE_SKEW_MS = 60 * 60 * 1000;
const RFC3339 = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(\.\d+)?(Z|[+-]\d{2}:\d{2})$/i;

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

/** The runtime market contract the desktop cache also enforces: an object with
 *  an RFC 3339 `updated_at` and a non-empty `items` object. A parseable body
 *  without prices must not replace real ones. Auxiliary maps stay optional and
 *  nullable. The local clock plays no part: a slow clock must not reject real
 *  snapshots. Release completeness is a separate, stricter check. */
export function isMarket(value: unknown): value is Market {
  if (!isRecord(value) || typeof value.updated_at !== 'string' || !RFC3339.test(value.updated_at)) return false;
  if (!Number.isFinite(Date.parse(value.updated_at))) return false;
  return isRecord(value.items) && Object.keys(value.items).length > 0;
}

/** Whether a snapshot stamped `next` should replace one stamped `current` at
 *  `nowMs`: when nothing usable is held, or `next` is strictly newer. A held
 *  stamp more than an hour ahead of the clock is no evidence of being newer;
 *  a 2099 stamp once blocked every later snapshot. */
export function replacesMarket(current: string | null | undefined, next: string, nowMs: number): boolean {
  const held = Date.parse(current ?? '');
  if (!Number.isFinite(held) || held - nowMs > MAX_FUTURE_SKEW_MS) return true;
  const incoming = Date.parse(next);
  return Number.isFinite(incoming) && incoming > held;
}
