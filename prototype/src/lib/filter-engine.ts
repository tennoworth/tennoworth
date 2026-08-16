// The Sell view's filter cascade — price / owned / type / kept-level / tag
// chips / preset-only clauses (vault-only, ducats-only, min-volume,
// min-median). Three call sites in App.svelte used to each re-implement this
// same clause sequence by hand (building the results rows, counting tag-chip
// availability, and diagnosing an empty result) — a 2026-07-24 god-object
// audit found the drift risk real, not hypothetical. Each `passes*` clause
// now exists in exactly one place; the three consumers below only differ in
// how they combine and report the clauses.

import { lookup } from './market';
import { clearingPrice, scoreRow, bandSignal, sellableQty, spareQty, LIQUID_VOL } from './sell-priority';
import type { Market, MarketItemEntry, OwnedRecord } from './types';

export interface FilterState {
  minPrice: number;
  minOwned: number;
  typeFilter: string;
  hideAtLvl: number;
  activeTags: Set<string>;
  /** From the active preset, if any — 0/false when no preset restricts these. */
  vaultOnly: boolean;
  ducatsOnly: boolean;
  minVol: number;
  minMedian: number;
  /** Preset-only: restrict to any of these row types (empty = no restriction). */
  typesAny: string[];
  /** Preset-only: Spares mode (see `spareQty`). */
  sparesOnly: boolean;
}

export interface EmptyReason {
  kind: string;
  excluded?: number;
  candidates: number;
  preset?: string | null;
}

function passesPrice(m: MarketItemEntry, f: FilterState): boolean {
  return m.avg >= f.minPrice;
}
function passesOwned(rec: OwnedRecord, f: FilterState): boolean {
  return rec.count >= f.minOwned;
}
function passesType(rec: OwnedRecord, f: FilterState): boolean {
  if (f.typesAny.length > 0 && !f.typesAny.includes(rec.type)) return false;
  return f.typeFilter === 'all' || rec.type === f.typeFilter;
}
function passesSpares(rec: OwnedRecord, f: FilterState): boolean {
  if (!f.sparesOnly) return true;
  return spareQty(rec.count, rec.kept_lvl, rec.leveled ?? 0) >= 1;
}
function passesKept(rec: OwnedRecord, f: FilterState): boolean {
  // null kept_lvl = no individualised instance at all (always show).
  return rec.kept_lvl === null || rec.kept_lvl < f.hideAtLvl;
}
function passesTag(m: MarketItemEntry, f: FilterState): boolean {
  if (f.activeTags.size === 0) return true;
  const tags = m.tags || [];
  return tags.some((t) => f.activeTags.has(t));
}
function passesVault(market: Market, rec: OwnedRecord, f: FilterState): boolean {
  if (!f.vaultOnly) return true;
  const status = market.vault_status?.[rec.slug];
  return status === 'vaulted' || status === 'vaulting-soon';
}
function passesDucats(rec: OwnedRecord, m: MarketItemEntry, f: FilterState): boolean {
  // The 'prime' tag alone also matches syndicate augments for prime weapons
  // (gilded_truth is tagged burston_prime), which have no ducat value — a
  // "best ducat value" list showing "Ducats: —" rows is nonsense.
  if (!f.ducatsOnly) return true;
  return !rec.subtype && m.ducats != null;
}
function passesVol(m: MarketItemEntry, f: FilterState): boolean {
  return f.minVol <= 0 || (m.vol || 0) >= f.minVol;
}
function passesMedian(m: MarketItemEntry, f: FilterState): boolean {
  return f.minMedian <= 0 || (m.median_90d || 0) >= f.minMedian;
}

// Row enrichment: score, timing, ducat-trade math. Runs only for rows that
// already passed every clause above.
function buildRow(key: string, rec: OwnedRecord, m: MarketItemEntry, market: Market, reserveCopies: number, sparesOnly = false) {
  const sellable = sparesOnly
    ? spareQty(rec.count, rec.kept_lvl, rec.leveled ?? 0)
    : sellableQty(rec.count, reserveCopies, rec.leveled ?? 0);
  const { sell_score, patience } = scoreRow({ owned: sellable, m });
  // ducats live on `m` because WFM is authoritative for the value —
  // warframestat's bulk /items/ endpoint doesn't carry it. Relics get
  // null so we don't suggest "Baro this" on a non-ducat trade.
  const ducats = rec.subtype ? null : (m.ducats ?? null);
  // p/100d — "platinum cost per 100 ducats of value." Low numbers mean
  // ducat-trading the part is the better deal vs selling it on WFM. Null
  // when no ducats data. Uses the clamped clearing price, not raw low_sell —
  // a single 1p troll ask made a stable 38p part read as a "feed it to
  // Baro" deal.
  const row_price = clearingPrice(m);
  const plat_per_100d = ducats && ducats > 0 && row_price > 0 ? (row_price * 100) / ducats : null;
  // 90d trend signal. `median_90d` is what experienced WFM traders price
  // against (48h avg is noisy on low-volume items). We compute Δ% vs the
  // 90d median using the most recent daily median as "now". Null when
  // there's no series yet (CSV-only rebuilds inherit zeros until the next
  // full scrape).
  const medians = Array.isArray(m.medians_7d) ? m.medians_7d.filter((v) => v > 0) : [];
  // "today" = latest daily median. Pre-split snapshots have no median_now,
  // so fall back to median_90d (which on those WAS the latest day). `||`
  // not `??`: a literal median_now of 0 is never a meaningful "today" price
  // (it's a thin item with no recent trade), so fall back to the 90d
  // baseline rather than null out the band + Δ signals entirely.
  const median_now = m.median_now || m.median_90d || null;
  // median_90d is now the 90-day BASELINE (median of the daily medians), so
  // Δ-vs-90d = today vs the 90-day norm — a real signal at last. On old
  // snapshots median_now === median_90d → Δ = 0 until the next scrape,
  // which is honest rather than fake.
  const median_90d = m.median_90d && m.median_90d > 0 ? m.median_90d : null;
  // A trend needs trades behind it: one closed sale can print ▲127% on a
  // vol-1 row (mountain's edge), which reads as signal but is one person's
  // afternoon. Below LIQUID_VOL the honest Δ is "not enough data" (null →
  // renders as ·), same bar the ask-clamp uses.
  const delta_90d_pct =
    median_now != null && median_90d != null && median_90d > 0 && (m.vol || 0) >= LIQUID_VOL
      ? ((median_now - median_90d) / median_90d) * 100
      : null;
  // Timing: where today's median sits in its 90-day band. Uses median_now,
  // not low_sell — the Donchian bands are built from the daily median
  // series, so a thin-book ask outlier (a lone 200p listing on a ~20p
  // item) would mislabel as "peak". median_now is always inside its own
  // band. "hold" = near the 90d low (don't dump into a trough — e.g. a mod
  // Baro just flooded), "peak" = near the 90d high (list now).
  const timing = bandSignal({
    price: median_now,
    donchTop: m.donch_top_90d,
    donchBot: m.donch_bot_90d,
    lowSell: m.low_sell,
    topBuy: m.top_buy,
  });
  const tags = Array.isArray(m.tags) ? m.tags : [];
  return {
    key,
    slug: rec.slug,
    subtype: rec.subtype ?? null,
    name: rec.name,
    owned: rec.count,
    sellable,
    leveled: rec.leveled ?? 0,
    type: rec.type,
    kept_lvl: rec.kept_lvl,
    ducats,
    plat_per_100d,
    avg_price: m.avg,
    low_sell: m.low_sell,
    // The sanity-clamped ask (what the score already prices at) — the
    // listing modal prefills from this, not raw low_sell, so a lone
    // fantasy ask can't become the suggested price.
    clearing_price: clearingPrice(m),
    low5_avg: m.low5_avg || 0,
    top_buy: m.top_buy,
    volume_48h: m.vol,
    ratio: m.ratio,
    potential_plat: sellable * m.avg,
    // Raw stack value: owned × the avg of the ~5 cheapest live asks — "what
    // is this pile worth at current listings", no liquidity discounting
    // (that's sell_score's job). Falls back to the 48h closed avg on
    // snapshots that predate low5_avg.
    raw_value: sellable * ((m.low5_avg || 0) > 0 ? (m.low5_avg as number) : m.avg),
    sell_score,
    patience,
    timing,
    medians_7d: medians,
    median_90d,
    delta_90d_pct,
    // Per-row metadata for the chip / badge surfaces. `tags` is already
    // the source of truth for filter chips; passing it on the row lets
    // ResultsTable render an [Aug] pill without re-looking-up the market
    // entry. vault_status drives the vault badge; absent = "available"
    // implicitly.
    tags,
    is_augment: tags.includes('augment'),
    vault_status: market.vault_status?.[rec.slug] ?? null,
  };
}

/** The Sell table's filtered + scored rows, best score first. */
export function computeResults(
  owned: Map<string, OwnedRecord>,
  market: Market | null | undefined,
  filters: FilterState,
  reserveCopies: number,
) {
  const out: ReturnType<typeof buildRow>[] = [];
  for (const [key, rec] of owned) {
    const m = lookup(market, rec.slug);
    if (!m) continue;
    if (!passesPrice(m, filters)) continue;
    if (!passesOwned(rec, filters)) continue;
    if (!passesType(rec, filters)) continue;
    if (!passesKept(rec, filters)) continue;
    if (!passesTag(m, filters)) continue;
    if (!market || !passesVault(market, rec, filters)) continue;
    if (!passesDucats(rec, m, filters)) continue;
    if (!passesVol(m, filters)) continue;
    if (!passesMedian(m, filters)) continue;
    if (!passesSpares(rec, filters)) continue;
    out.push(buildRow(key, rec, m, market, reserveCopies, filters.sparesOnly));
  }
  out.sort((a, b) => b.sell_score - a.sell_score);
  return out;
}

/** Tag → count, for the chip row. Mirrors every clause `computeResults`
 * applies EXCEPT the tag clause itself — otherwise chip counts would
 * overstate what clicking actually yields. */
export function computeAvailableTags(
  owned: Map<string, OwnedRecord>,
  market: Market | null | undefined,
  filters: FilterState,
): Array<[string, number]> {
  const counts = new Map<string, number>();
  for (const rec of owned.values()) {
    const m = lookup(market, rec.slug);
    if (!m) continue;
    if (!passesPrice(m, filters)) continue;
    if (!passesOwned(rec, filters)) continue;
    if (!passesType(rec, filters)) continue;
    if (!passesKept(rec, filters)) continue;
    if (!market || !passesVault(market, rec, filters)) continue;
    if (!passesDucats(rec, m, filters)) continue;
    if (!passesVol(m, filters)) continue;
    if (!passesMedian(m, filters)) continue;
    if (!passesSpares(rec, filters)) continue;
    for (const t of m.tags || []) counts.set(t, (counts.get(t) || 0) + 1);
  }
  return [...counts.entries()].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]));
}

/** Why the Sell table is empty: which single clause excludes the most
 * candidates, for the empty-state's "relax this filter" suggestion. Walks
 * every clause independently (not short-circuited) so a row can count
 * against more than one bucket — this is a diagnostic, not a filter. */
export function computeEmptyReason(
  owned: Map<string, OwnedRecord>,
  market: Market | null | undefined,
  filters: FilterState,
  resultsLength: number,
  activePreset: string | null,
): EmptyReason | null {
  if (resultsLength > 0 || !owned.size) return null;
  let candidates = 0, byPrice = 0, byOwned = 0, byType = 0, byKept = 0,
      byTag = 0, byVault = 0, byDucats = 0, byVol = 0, byMedian = 0, bySpares = 0;
  for (const rec of owned.values()) {
    const m = lookup(market, rec.slug);
    if (!m) continue;
    candidates += 1;
    if (!passesPrice(m, filters)) byPrice += 1;
    if (!passesOwned(rec, filters)) byOwned += 1;
    if (!passesType(rec, filters)) byType += 1;
    if (!passesKept(rec, filters)) byKept += 1;
    if (!passesTag(m, filters)) byTag += 1;
    if (market && !passesVault(market, rec, filters)) byVault += 1;
    if (!passesDucats(rec, m, filters)) byDucats += 1;
    if (!passesVol(m, filters)) byVol += 1;
    if (!passesMedian(m, filters)) byMedian += 1;
    if (!passesSpares(rec, filters)) bySpares += 1;
  }
  if (candidates === 0) return { kind: 'no-market', candidates };
  const top = ([
    ['price', byPrice], ['owned', byOwned], ['type', byType], ['kept', byKept],
    ['tag', byTag], ['vault', byVault], ['ducats', byDucats], ['vol', byVol], ['median', byMedian],
    ['spares', bySpares],
  ] as Array<[string, number]>).sort((a, b) => b[1] - a[1])[0];
  return { kind: top[0], excluded: top[1], candidates, preset: activePreset };
}
