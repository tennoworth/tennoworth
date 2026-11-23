// The Sell view's filter cascade - price / owned / type / kept-level / tag
// chips / preset-only clauses. The clauses are native
// (market_domain::sell_filter), each existing once there; the table's rows,
// the chip counts and the empty-state diagnosis are one call each over the
// whole inventory. This file joins each owned record to its market entry and
// native fact for that call, and builds the table's rows from the answer.

import type { ScoredInventoryFact, Verdict } from '../contracts/generated/domain';
import type { SellFilterRow, EmptyReason as NativeEmptyReason } from '../contracts/generated/web';
import { lookup } from './market';
import { crowdedBook } from './book-depth';
import { call, jsNumber } from './native';
import { rememberTagGroups } from './tag-groups';
import type { Market, MarketItemEntry, OwnedRecord } from '../contracts/data';

export interface FilterState {
  minPrice: number;
  minOwned: number;
  typeFilter: string;
  hideAtLvl: number;
  activeTags: Set<string>;
  /** From the active preset, if any - 0/false when no preset restricts these. */
  vaultOnly: boolean;
  ducatsOnly: boolean;
  minVol: number;
  minMedian: number;
  /** Preset-only: restrict to any of these row types (empty = no restriction). */
  typesAny: string[];
  /** Preset-only: Spares mode (market_domain::math::sell_priority::spare_qty). */
  sparesOnly: boolean;
  /** Preset-only: only rows the advisor has a verdict for. */
  adviceOnly: boolean;
}

export type EmptyReason = NativeEmptyReason & { preset?: string | null };

interface Joined { key: string; rec: OwnedRecord; m: MarketItemEntry }

const filtersFor = (f: FilterState) => ({
  min_price: jsNumber(f.minPrice), min_owned: jsNumber(f.minOwned), type_filter: f.typeFilter,
  hide_at_lvl: jsNumber(f.hideAtLvl), active_tags: [...f.activeTags], vault_only: !!f.vaultOnly,
  ducats_only: !!f.ducatsOnly, min_vol: jsNumber(f.minVol), min_median: jsNumber(f.minMedian),
  types_any: f.typesAny, spares_only: !!f.sparesOnly, advice_only: !!f.adviceOnly,
});

// The records with a market entry, each with the fields the clauses read. An
// inventory shares a few hundred market tags across its rows, so each tag
// travels once and rows name theirs by index.
function join(owned: Map<string, OwnedRecord>, market: Market | null | undefined, advice?: Map<string, Verdict>) {
  const joined: Joined[] = [];
  const rows: SellFilterRow[] = [];
  const tags: string[] = [];
  const tagIds = new Map<string, number>();
  const intern = (tag: string) => {
    let id = tagIds.get(tag);
    if (id === undefined) { id = tags.length; tags.push(tag); tagIds.set(tag, id); }
    return id;
  };
  for (const [key, rec] of owned) {
    const m = lookup(market, rec.slug);
    if (!m) continue;
    joined.push({ key, rec, m });
    rows.push({
      count: jsNumber(rec.count), type: rec.type ?? null,
      kept_lvl: rec.kept_lvl === null ? null : jsNumber(rec.kept_lvl), leveled: jsNumber(rec.leveled ?? 0),
      subtype: rec.subtype ?? null, avg: jsNumber(m.avg), vol: jsNumber(m.vol), median_90d: jsNumber(m.median_90d),
      low5_avg: jsNumber(m.low5_avg), ducats: m.ducats == null ? null : jsNumber(m.ducats), tags: (m.tags ?? []).map(intern),
      vault: market?.vault_status?.[rec.slug] ?? null, advice: advice?.get(rec.slug)?.advice ?? null,
    });
  }
  return { joined, rows, tags };
}

function buildRow({ key, rec, m }: Joined, market: Market | null | undefined, fact: ScoredInventoryFact, native: { sellable: number; potential_plat: number; raw_value: number; timing: ScoredInventoryFact['timing'] }, verdict?: Verdict) {
  const { clearing_price: row_price, sell_score, patience, ducats, plat_per_100d,
    medians_7d: medians, median_90d, delta_90d_pct, demand } = fact;
  const tags = Array.isArray(m.tags) ? m.tags : [];
  return {
    key,
    slug: rec.slug,
    subtype: rec.subtype ?? null,
    name: rec.name,
    owned: rec.count,
    sellable: native.sellable,
    leveled: rec.leveled ?? 0,
    type: rec.type,
    kept_lvl: rec.kept_lvl,
    ducats,
    plat_per_100d,
    avg_price: m.avg,
    low_sell: m.low_sell,
    // The sanity-clamped ask (what the score already prices at) - the
    // listing modal prefills from this, not raw low_sell, so a lone
    // fantasy ask can't become the suggested price.
    clearing_price: row_price,
    low5_avg: m.low5_avg || 0,
    top_buy: m.top_buy,
    volume_48h: m.vol,
    ratio: m.ratio,
    potential_plat: native.potential_plat,
    // Raw stack value: owned × the avg of the ~5 cheapest live asks - "what
    // is this pile worth at current listings", no liquidity discounting
    // (that's sell_score's job).
    raw_value: native.raw_value,
    sell_score,
    patience,
    crowded: crowdedBook(m),
    timing: native.timing,
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
    vault_status: market?.vault_status?.[rec.slug] ?? null,
    // Advisor verdict for calendar-dated primes; null on everything else.
    advice: verdict?.advice ?? null,
    advice_reasons: verdict?.reasons ?? [],
    // Resolved once per row and reused by both scoring and DemandCell.
    demand,
  };
}

/** The Sell table's filtered + scored rows, best score first. */
export function computeResults(
  owned: Map<string, OwnedRecord>,
  market: Market | null | undefined,
  filters: FilterState,
  // score_inventory's rows for this inventory, reserve and spares mode. A row
  // without one is not shown: the table never scores on its own.
  nativeFacts: ReadonlyMap<string, ScoredInventoryFact>,
  advice?: Map<string, Verdict>,
  availability?: ReadonlyMap<string, number>,
) {
  const { joined, rows, tags } = join(owned, market, advice);
  if (rows.length === 0) return [];
  joined.forEach(({ key }, i) => {
    const fact = nativeFacts.get(key);
    if (fact) rows[i]!.fact = { sellable: jsNumber(fact.sellable), sell_score: jsNumber(fact.sell_score), timing: fact.timing };
    if (availability) rows[i]!.allocation = jsNumber(availability.get(key) ?? 0);
  });
  return call({ op: 'sell_results', input: { tags, rows, filters: filtersFor(filters) } }).map((native) => {
    const row = joined[native.index]!;
    return buildRow(row, market, nativeFacts.get(row.key)!, native, advice?.get(row.rec.slug));
  });
}

/** Tag → count, for the chip row: every clause `computeResults` applies
 *  except the tag clause itself, so a chip's count is what clicking it
 *  yields. Most common first, then by name. */
export function computeAvailableTags(
  owned: Map<string, OwnedRecord>,
  market: Market | null | undefined,
  filters: FilterState,
  advice?: Map<string, Verdict>,
): Array<[string, number]> {
  const { rows, tags } = join(owned, market, advice);
  if (rows.length === 0) return [];
  const available = call({ op: 'sell_tags', input: { tags, rows, filters: filtersFor(filters) } });
  rememberTagGroups(available);
  // The name order is the locale's, so it stays with the page.
  return available.map(({ tag, count }): [string, number] => [tag, count]).sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]));
}

/** Why the Sell table is empty: which single clause excludes the most
 * candidates, for the empty-state's "relax this filter" suggestion. */
export function computeEmptyReason(
  owned: Map<string, OwnedRecord>,
  market: Market | null | undefined,
  filters: FilterState,
  resultsLength: number,
  activePreset: string | null,
  advice?: Map<string, Verdict>,
): EmptyReason | null {
  if (resultsLength > 0 || !owned.size) return null;
  const { rows, tags } = join(owned, market, advice);
  const reason = call({ op: 'sell_empty_reason', input: { tags, rows, filters: filtersFor(filters) } });
  return reason.kind === 'no-market' ? reason : { ...reason, preset: activePreset };
}
