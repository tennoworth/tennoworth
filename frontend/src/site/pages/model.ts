import type { PublicMarket } from '../../contracts/data';
import { buildBrowseIndex } from '../../domain/market-browse';
import { itemPath, pilotSets } from '../../features/market/pages';

export interface PublicItem {
  slug: string; name: string; price: number | null; volume: number | null;
  ducats: number | null; ducatPart: boolean; prime: boolean; baseline: number | null; recent: number[];
}
export interface Comparison { slug: string; name: string; parts: { slug: string; quantity: number | null }[] }
export interface PublicData { stamp: string; platform: string; items: PublicItem[]; sets: Comparison[] }
const positive = (n: unknown): number | null => typeof n === 'number' && Number.isFinite(n) && n > 0 ? n : null;
export function publicData(market: PublicMarket, slugs?: string[]): PublicData {
  const names = buildBrowseIndex(market);
  const selected = slugs ? new Set(slugs) : null;
  const sets = market.set_to_parts ?? {};
  const components = new Set(Object.values(sets).flatMap(set => set.parts.map(part => part.slug)));
  return {
    stamp: market.updated_at, platform: market.platform,
    items: Object.entries(market.items).filter(([slug]) => !selected || selected.has(slug)).map(([slug, item]) => ({
      slug, name: names.nameOf(slug), price: item.price_basis === 'unit' ? positive(item.median_now) : null,
      volume: typeof item.vol === 'number' && Number.isFinite(item.vol) && item.vol >= 0 ? item.vol : null,
      ducats: positive(item.ducats),
      prime: Array.isArray(item.tags) && item.tags.includes('prime'),
      ducatPart: positive(item.ducats) !== null && Array.isArray(item.tags) && item.tags.includes('prime')
        && !item.tags.includes('set') && !(slug in sets)
        && (item.tags.includes('blueprint') || item.tags.includes('component') || components.has(slug)),
      baseline: item.price_basis === 'unit' ? positive(item.median_90d) : null,
      recent: item.price_basis === 'unit' ? (item.medians_7d ?? []).filter(n => positive(n) !== null) : [],
    })),
    sets: pilotSets.flatMap(slug => {
      const set = market.set_to_parts?.[slug];
      return set ? [{ slug, name: set.name, parts: set.parts.map(p => ({ slug: p.slug, quantity: Number.isInteger(p.quantity) ? positive(p.quantity) : null })) }] : [];
    }),
  };
}
export function partTotal(set: Comparison, data: PublicData): number | null {
  if (!set.parts.length) return null;
  let total = 0;
  for (const part of set.parts) {
    const price = data.items.find(item => item.slug === part.slug)?.price;
    if (price == null || part.quantity == null) return null;
    total += price * part.quantity;
  }
  return total;
}


export type DecisionOutcome = 'set-higher' | 'parts-higher' | 'tie' | 'unavailable';
export type DecisionReason = 'missing-data' | 'incomparable-data' | 'invalid-snapshot' | null;

export interface DecisionPart {
  slug: string;
  name: string;
  price: number | null;
  quantity: number | null;
}

export interface DecisionAnswer {
  slug: string;
  name: string;
  url: string;
  setOrdersUrl: string;
  stamp: string;
  platform: string;
  setPrice: number | null;
  partsTotal: number | null;
  componentCopies: number | null;
  distinctComponents: number;
  setVolume48h: number | null;
  gap: number | null;
  outcome: DecisionOutcome;
  reason: DecisionReason;
  missing: string[];
  parts: DecisionPart[];
}

const MONTHS = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'];

/** Stable UTC day label; avoids locale-dependent SSR/hydration text. */
export function formatStampUtc(stamp: string): string {
  const date = new Date(stamp);
  if (!Number.isFinite(date.getTime())) return 'Unknown date';
  return `${String(date.getUTCDate()).padStart(2, '0')} ${MONTHS[date.getUTCMonth()]} ${date.getUTCFullYear()}`;
}

export function formatStampUtcTime(stamp: string): string {
  const date = new Date(stamp);
  if (!Number.isFinite(date.getTime())) return 'Unknown time';
  return `${formatStampUtc(stamp)} ${String(date.getUTCHours()).padStart(2, '0')}:${String(date.getUTCMinutes()).padStart(2, '0')} UTC`;
}

export function priceText(value: number | null | undefined): string {
  return value == null ? 'Unavailable' : `${Number(value.toFixed(2))}p`;
}

/**
 * The public set-versus-parts answer. It deliberately does not claim which
 * sells faster: the snapshot has no per-item observation date and the parts
 * figure is a quantity-weighted sum of component medians, not a bundle median.
 */
export function decisionAnswer(data: PublicData, slug: string): DecisionAnswer | null {
  if (!pilotSets.includes(slug)) return null;
  const item = (partSlug: string) => data.items.find(candidate => candidate.slug === partSlug);
  const setItem = item(slug);
  const set = data.sets.find(candidate => candidate.slug === slug) ?? { slug, name: setItem?.name ?? slug.replaceAll('_', ' '), parts: [] };
  const name = setItem?.name ?? set.name;
  const parts: DecisionPart[] = set.parts.map(part => ({
    slug: part.slug,
    name: item(part.slug)?.name ?? part.slug.replaceAll('_', ' '),
    price: item(part.slug)?.price ?? null,
    quantity: part.quantity,
  }));
  const missing = parts.filter(part => part.price == null || part.quantity == null).map(part => part.name);
  if (setItem?.price == null) missing.unshift(name);
  const duplicate = new Set(parts.map(part => part.slug)).size !== parts.length;
  if (duplicate) missing.push('duplicate component mapping');
  const componentCopies = parts.length > 0 && parts.every(part => part.quantity != null)
    ? parts.reduce((total, part) => total + (part.quantity ?? 0), 0)
    : null;
  let partsTotal = parts.length > 0 && parts.every(part => part.price != null && part.quantity != null)
    ? parts.reduce((total, part) => total + (part.price ?? 0) * (part.quantity ?? 0), 0)
    : null;
  const nonfinite = partsTotal != null && !Number.isFinite(partsTotal);
  if (nonfinite) partsTotal = null;
  const setPrice = setItem?.price ?? null;
  const validStamp = Number.isFinite(Date.parse(data.stamp));
  const platformOk = data.platform.toLowerCase() === 'pc';
  let reason: DecisionReason = null;
  if (!validStamp) reason = 'invalid-snapshot';
  else if (!platformOk || duplicate || nonfinite) reason = 'incomparable-data';
  else if (setPrice == null || partsTotal == null) reason = 'missing-data';
  const gap = setPrice == null || partsTotal == null ? null : setPrice - partsTotal;
  const outcome: DecisionOutcome = reason ? 'unavailable'
    : gap == null ? 'unavailable'
    : gap > 0 ? 'set-higher'
    : gap < 0 ? 'parts-higher'
    : 'tie';
  return {
    slug, name, url: `https://tennoworth.app${itemPath(slug)}`,
    setOrdersUrl: `https://warframe.market/items/${slug}`,
    stamp: data.stamp, platform: data.platform.toUpperCase(),
    setPrice, partsTotal, componentCopies, distinctComponents: parts.length,
    setVolume48h: setItem?.volume ?? null,
    gap, outcome, reason, missing, parts,
  };
}

export function decisionText(answer: DecisionAnswer): {
  headline: string;
  quantities: string;
  evidence: string;
  clipboard: string;
  ogTitle: string;
  ogDescription: string;
} {
  const day = formatStampUtc(answer.stamp);
  const stamp = formatStampUtcTime(answer.stamp);
  const gapText = answer.gap == null ? null : `${Number(Math.abs(answer.gap).toFixed(2))}`;
  const headline = answer.outcome === 'unavailable'
    ? 'Not enough comparable evidence for a set-versus-parts verdict.'
    : answer.outcome === 'set-higher'
      ? `The set is ${gapText}p above the summed parts.`
      : answer.outcome === 'parts-higher'
        ? `The summed parts are ${gapText}p above the set.`
        : `The set and summed parts are equal at ${priceText(answer.setPrice)}.`;
  const copies = answer.componentCopies == null
    ? 'an unknown number of component copies'
    : `${answer.componentCopies} component ${answer.componentCopies === 1 ? 'copy' : 'copies'}`;
  const quantities = `One set versus ${copies} across ${answer.distinctComponents} distinct ${answer.distinctComponents === 1 ? 'item' : 'items'}.`;
  const volume = answer.setVolume48h == null ? 'Set reported volume: unavailable.' : `Set reported volume: ${answer.setVolume48h}/48h.`;
  const evidence = `Snapshot ${stamp} · ${answer.platform} · ${volume} Source: warframe.market. Latest daily medians; individual observation dates unavailable. Parts total = quantity-weighted sum of component medians, not a bundle median. Estimates, not live offers; data may be stale. This does not establish which sells faster.`;
  const reason = answer.reason === 'invalid-snapshot' ? 'snapshot timestamp unavailable'
    : answer.reason === 'incomparable-data' ? 'snapshot platform is not comparable'
    : answer.reason === 'missing-data' ? `missing price or quantity${answer.missing.length ? `: ${answer.missing.join(', ')}` : ''}`
    : 'evidence unavailable';
  const clipboard = answer.outcome === 'unavailable'
    ? `${answer.name}: set-versus-parts comparison unavailable (${reason}) at snapshot ${stamp}, ${answer.platform}.\nLatest daily medians; estimates, not live offers; not evidence of sale speed.\n${answer.url}`
    : `${answer.name}: set ${priceText(answer.setPrice)} vs ${priceText(answer.partsTotal)} summed parts (snapshot ${stamp}, ${answer.platform}).\n${headline} ${quantities}\nLatest daily medians; estimates, not live offers; not evidence of sale speed.\n${answer.url}`;
  const ogTitle = answer.outcome === 'unavailable'
    ? `${answer.name}: set-versus-parts comparison`
    : `${answer.name}: ${priceText(answer.setPrice)} set vs ${priceText(answer.partsTotal)} summed parts (${day})`;
  const ogDescription = answer.outcome === 'unavailable'
    ? `The set-versus-parts comparison is unavailable (${reason}). ${answer.platform} snapshot ${day}; estimates, not live offers.`
    : `${headline} ${quantities} Snapshot ${day} · ${answer.platform}. Latest daily medians; estimates, not live offers.`;
  return { headline, quantities, evidence, clipboard, ogTitle, ogDescription };
}

export function ducatParts(data: PublicData): PublicItem[] {
  return data.items.filter(item => item.ducatPart && item.ducats !== null && item.price !== null);
}

export function activityCandidates(data: PublicData): PublicItem[] {
  return data.items.filter(item => item.prime && item.price !== null && item.volume !== null && item.volume > 0)
    .sort((a, b) => b.volume! - a.volume! || a.slug.localeCompare(b.slug)).slice(0, 8);
}

interface DatedObservation { date: string; median: number | null; volume: number | null }
export function datedHistory(value: unknown, slug: string): { through: string; generated: string; rows: DatedObservation[] } | null {
  if (!value || typeof value !== 'object') return null;
  const h = value as Record<string, unknown>;
  if (typeof h.start !== 'string' || !/^\d{4}-\d{2}-\d{2}$/.test(h.start)
    || !Number.isInteger(h.days) || (h.days as number) < 1 || (h.days as number) > 4000
    || typeof h.generated_at !== 'string' || !Number.isFinite(Date.parse(h.generated_at))
    || !h.items || typeof h.items !== 'object' || Array.isArray(h.items)) return null;
  const start = Date.parse(h.start + 'T00:00:00Z');
  if (!Number.isFinite(start) || new Date(start).toISOString().slice(0, 10) !== h.start) return null;
  const days = h.days as number;
  const end = new Date(start + (days - 1) * 86_400_000).toISOString().slice(0, 10);
  const through = h.through ?? end;
  const throughMs = typeof through === 'string' ? Date.parse(through + 'T00:00:00Z') : NaN;
  if (!Number.isFinite(throughMs) || typeof through !== 'string' || !/^\d{4}-\d{2}-\d{2}$/.test(through)
    || through < h.start || through > end || new Date(throughMs).toISOString().slice(0, 10) !== through) return null;
  const series = (h.items as Record<string, unknown>)[slug] as Record<string, unknown> | undefined;
  if (!series || !Array.isArray(series.median) || !Array.isArray(series.volume)
    || series.median.length !== days || series.volume.length !== days || series.subtype != null) return null;
  if (h.missing_days != null && (!Array.isArray(h.missing_days) || h.missing_days.some(day => typeof day !== 'string'))) return null;
  const missing = new Set((h.missing_days ?? []) as string[]);
  const rows: DatedObservation[] = [];
  for (let i = 0; i < days; i++) {
    const date = new Date(start + i * 86_400_000).toISOString().slice(0, 10);
    if (date > through) break;
    const volume = series.volume[i];
    rows.push({ date, median: missing.has(date) ? null : positive(series.median[i]),
      volume: missing.has(date) || typeof volume !== 'number' || !Number.isFinite(volume) || volume < 0 ? null : volume });
  }
  return { through, generated: h.generated_at, rows: rows.reverse() };
}
