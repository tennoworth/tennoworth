// The no-inventory landing browser's lists (MarketBrowser.svelte). Each list
// is one native call (market_domain::browse, documented there) over the rows
// of the baked market.json it reads; this file sends those rows in snapshot
// order and builds the rendered rows from the snapshot's own entries.

import type { Market, MarketItemEntry, RivenDispoChange, VaultStatus } from '../contracts/data';
import { call, native } from './native';

/** A single item row rendered by the browser (search, movers, vaulted). */
export interface BrowseRow {
  slug: string;
  name: string;
  avg: number;
  vol: number;
  medians_7d?: number[];
  vault?: VaultStatus;
  // Move vs the 90-day baseline, in percent. null when not computable.
  deltaPct: number | null;
  // The 90-day median itself, drawn as the sparkline's reference line.
  median90: number | null;
  // Top-of-book + demand + ducats, so the landing tables can carry the same
  // columns as the workspace (Low ask · Top buy · Demand · Ducats).
  lowSell: number;
  topBuy: number;
  ratio: number;
  ducats: number | null;
}

/** Reverse-catalog + name lookup, built once from a snapshot. */
export interface BrowseIndex {
  // One entry per priceable catalog item, for type-ahead search.
  names: Array<{ slug: string; name: string; nameLower: string }>;
  nameOf(slug: string): string;
}

type Items = Record<string, unknown> | null | undefined;
const asNumber = (v: unknown): number | null => typeof v === 'number' ? v : null;
// What `<` and `-` read; JSON carries NaN as null, which the rules read as NaN.
const toNumber = (v: unknown): number => Number(v);
const positive = (v: unknown): number | null => typeof v === 'number' && v > 0 ? v : null;

export function buildBrowseIndex(market: { catalog?: Market['catalog'] | null; items?: Items } | null | undefined): BrowseIndex {
  const items = market?.items;
  const { names, labels } = call({ op: 'browse_index', input: {
    catalog: market?.catalog ? Object.entries(market.catalog) : null,
    items: items ? Object.entries(items).map(([slug, e]) => [slug, !!e]) : null,
  } });
  const bySlug = new Map(labels);
  return {
    names,
    nameOf: (slug) => bySlug.get(slug) ?? call({ op: 'slug_name', input: slug }),
  };
}

function toRow(index: BrowseIndex, slug: string, e: MarketItemEntry, vault?: VaultStatus): BrowseRow {
  const delta = native().web_delta_pct(asNumber(e.median_now) ?? Number.NaN, asNumber(e.median_90d) ?? Number.NaN);
  return {
    slug,
    name: index.nameOf(slug),
    avg: e.avg,
    vol: e.vol,
    medians_7d: e.medians_7d,
    vault,
    deltaPct: Number.isNaN(delta) ? null : delta,
    median90: positive(e.median_90d),
    lowSell: e.low_sell,
    topBuy: e.top_buy,
    ratio: e.ratio,
    ducats: positive(e.ducats),
  };
}

export function searchItems(
  market: Market | null | undefined,
  index: BrowseIndex,
  query: string,
  limit = 12
): BrowseRow[] {
  const q = query.trim().toLowerCase();
  const items = market?.items;
  if (!q || !items) return [];
  const vault = market?.vault_status;
  // One call per keystroke over every priced name. Measured 2026-11-23 against
  // the real market.json (3,893 names): ~1 ms with the request's encoding.
  const listed = index.names.filter((n) => items[n.slug]);
  const hits = call({ op: 'search_items', input: {
    query: q, limit, names: listed.map((n) => [n.nameLower, toNumber(items[n.slug]!.vol)]),
  } });
  return hits.map((i) => {
    const { slug } = listed[i]!;
    return toRow(index, slug, items[slug]!, vault?.[slug]);
  });
}

export interface MoversOpts {
  minVol?: number;
  minPrice?: number;
  limit?: number;
}

export function topMovers(
  market: Market | null | undefined,
  index: BrowseIndex,
  opts: MoversOpts = {}
): { risers: BrowseRow[]; fallers: BrowseRow[] } {
  const items = market?.items;
  if (!items) return { risers: [], fallers: [] };
  const vault = market?.vault_status;
  const entries = Object.entries(items).filter(([, e]) => e);
  const { risers, fallers } = call({ op: 'top_movers', input: {
    items: entries.map(([, e]) => ({
      avg: asNumber(e.avg), vol: toNumber(e.vol), median_now: asNumber(e.median_now), median_90d: asNumber(e.median_90d),
    })),
    ...(opts.minVol === undefined ? {} : { min_vol: opts.minVol }),
    ...(opts.minPrice === undefined ? {} : { min_price: opts.minPrice }),
    ...(opts.limit === undefined ? {} : { limit: opts.limit }),
  } });
  const rows = (at: number[]) => at.map((i) => {
    const [slug, e] = entries[i]!;
    return toRow(index, slug, e, vault?.[slug]);
  });
  return { risers: rows(risers), fallers: rows(fallers) };
}

// The highest-value currently-vaulted items (vault_status × items).
export function vaultedTop(
  market: Market | null | undefined,
  index: BrowseIndex,
  limit = 12
): BrowseRow[] {
  const items = market?.items;
  const vault = market?.vault_status;
  if (!items || !vault) return [];
  const entries = Object.entries(vault);
  const picked = call({ op: 'vaulted_top', input: {
    rows: entries.map(([slug, status]) => ({
      status: typeof status === 'string' ? status : null,
      avg: items[slug] ? asNumber(items[slug].avg) : null,
    })),
    limit,
  } });
  return picked.map((i) => {
    const [slug, status] = entries[i]!;
    return toRow(index, slug, items[slug]!, status);
  });
}

/** Disposition changes from the snapshot's rolling log, newest first, capped. */
export function dispositionChanges(market: Market | null | undefined, limit = 12): RivenDispoChange[] {
  const changes = market?.rivens?.changes;
  if (!Array.isArray(changes)) return [];
  return call({ op: 'disposition_changes', input: { changes, limit } }).map((i) => changes[i]!);
}

/** A hand-off row: a market row plus the three numbers only the desktop app can
 *  fill (owned · priority · stack value), with SAMPLE owned counts so the
 *  landing can show what a completed row looks like. The pipeline picks the
 *  rows and scores them with the desktop's priority (`handoff_sample`); stack
 *  value remains owned × avg. */
export interface HandoffRow extends BrowseRow {
  owned: number;
  score: number;
  potential: number;
}

export function handoffSample(
  market: Market | null | undefined,
  index: BrowseIndex,
): HandoffRow[] {
  const items = market?.items;
  const picks = market?.handoff_sample;
  if (!items || !Array.isArray(picks)) return [];
  const backed = picks.filter((pick) => pick && typeof pick.slug === 'string' && items[pick.slug]);
  const kept = call({ op: 'handoff_sample', input: {
    picks: backed.map((pick) => ({ owned: pick.owned, score: pick.score, avg: toNumber(items[pick.slug]!.avg) })),
  } });
  return kept.map(({ at, owned, score, potential }) => {
    const { slug } = backed[at]!;
    // A non-numeric avg multiplies to NaN, which JSON carries as null.
    return { ...toRow(index, slug, items[slug]!, market?.vault_status?.[slug]), owned, score, potential: potential ?? Number.NaN };
  });
}
