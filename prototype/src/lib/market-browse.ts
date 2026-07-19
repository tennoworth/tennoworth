// Pure data-shaping for the no-inventory landing browser (MarketBrowser.svelte).
// Everything here reads ONLY the baked market.json snapshot — no fetches, no
// DOM — so the search / movers / vault joins stay unit-testable in isolation.

import type { Market, MarketItemEntry, VaultStatus } from './types';

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
}

/** Reverse-catalog + name lookup, built once from a snapshot. */
export interface BrowseIndex {
  // One entry per priceable catalog item, for type-ahead search.
  names: Array<{ slug: string; name: string; nameLower: string }>;
  nameOf(slug: string): string;
}

// The catalog stores display names lowercased (it's the WFM name→slug join
// key). We only need a readable label, so upper-case each word's first char.
export function titleCase(name: string): string {
  return name.replace(/\b\w/g, (c) => c.toUpperCase());
}

export function buildBrowseIndex(market: Market | null | undefined): BrowseIndex {
  const names: BrowseIndex['names'] = [];
  const bySlug = new Map<string, string>();
  const catalog = market?.catalog;
  const items = market?.items;
  if (catalog) {
    for (const [nameLower, slug] of Object.entries(catalog)) {
      // Only surface items we can actually price — search is a sell tool, an
      // un-priceable quest key is noise.
      if (items && !items[slug]) continue;
      const name = titleCase(nameLower);
      bySlug.set(slug, name);
      names.push({ slug, name, nameLower });
    }
  }
  return {
    names,
    nameOf: (slug) => bySlug.get(slug) ?? titleCase(slug.replace(/_/g, ' ')),
  };
}

// Δ% vs the 90-day baseline. null when it can't be computed meaningfully:
// missing median_now, or a missing/zero median_90d (dividing by it is junk —
// pre-split snapshots and brand-new items land here).
export function itemDeltaPct(e: MarketItemEntry | null | undefined): number | null {
  if (!e) return null;
  const base = e.median_90d;
  const now = e.median_now;
  if (typeof base !== 'number' || base <= 0) return null;
  if (typeof now !== 'number') return null;
  return ((now - base) / base) * 100;
}

function toRow(
  index: BrowseIndex,
  slug: string,
  e: MarketItemEntry,
  vault?: VaultStatus
): BrowseRow {
  return {
    slug,
    name: index.nameOf(slug),
    avg: e.avg,
    vol: e.vol,
    medians_7d: e.medians_7d,
    vault,
    deltaPct: itemDeltaPct(e),
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
  // Rank prefix matches above mid-word substring matches, then by volume, so
  // "primed" surfaces the "Primed …" mods before "… Primed …" parts.
  const starts: BrowseRow[] = [];
  const contains: BrowseRow[] = [];
  for (const { slug, nameLower } of index.names) {
    const at = nameLower.indexOf(q);
    if (at < 0) continue;
    const e = items[slug];
    if (!e) continue;
    (at === 0 ? starts : contains).push(toRow(index, slug, e, vault?.[slug]));
  }
  starts.sort((a, b) => b.vol - a.vol);
  contains.sort((a, b) => b.vol - a.vol);
  return starts.concat(contains).slice(0, limit);
}

export interface MoversOpts {
  minVol?: number;
  minPrice?: number;
  limit?: number;
}

// Top risers/fallers by Δ% vs the 90-day median. Two floors keep the flagship
// list honest: the volume floor drops thin books (a 200% "move" on 3 trades is
// noise), and the price floor drops cheap junk (a ±100% swing on a 3p relic
// isn't worth plat — the move has to be on an item where plat is at stake).
export function topMovers(
  market: Market | null | undefined,
  index: BrowseIndex,
  opts: MoversOpts = {}
): { risers: BrowseRow[]; fallers: BrowseRow[] } {
  const minVol = opts.minVol ?? 20;
  const minPrice = opts.minPrice ?? 10;
  const limit = opts.limit ?? 8;
  const items = market?.items;
  if (!items) return { risers: [], fallers: [] };
  const vault = market?.vault_status;
  const rows: BrowseRow[] = [];
  for (const [slug, e] of Object.entries(items)) {
    if (!e || e.vol < minVol) continue;
    if (typeof e.avg !== 'number' || e.avg < minPrice) continue;
    const d = itemDeltaPct(e);
    if (d == null || d === 0) continue;
    rows.push(toRow(index, slug, e, vault?.[slug]));
  }
  const risers = rows
    .filter((r) => (r.deltaPct as number) > 0)
    .sort((a, b) => (b.deltaPct as number) - (a.deltaPct as number))
    .slice(0, limit);
  const fallers = rows
    .filter((r) => (r.deltaPct as number) < 0)
    .sort((a, b) => (a.deltaPct as number) - (b.deltaPct as number))
    .slice(0, limit);
  return { risers, fallers };
}

// Highest-value currently-vaulted items (vault_status × items). Only 'vaulted'
// — 'vaulting-soon' and 'available' are different signals.
export function vaultedTop(
  market: Market | null | undefined,
  index: BrowseIndex,
  limit = 12
): BrowseRow[] {
  const items = market?.items;
  const vault = market?.vault_status;
  if (!items || !vault) return [];
  const rows: BrowseRow[] = [];
  for (const [slug, status] of Object.entries(vault)) {
    if (status !== 'vaulted') continue;
    const e = items[slug];
    if (!e || typeof e.avg !== 'number' || e.avg <= 0) continue;
    rows.push(toRow(index, slug, e, status));
  }
  rows.sort((a, b) => b.avg - a.avg);
  return rows.slice(0, limit);
}
