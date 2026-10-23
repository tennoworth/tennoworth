import type { Market } from '../contracts/data';
import { buildBrowseIndex } from '../domain/market-browse';
import { pilotSets } from './pages';

export interface PublicItem {
  slug: string; name: string; price: number | null; volume: number | null;
  ducats: number | null; ducatPart: boolean; prime: boolean; baseline: number | null; recent: number[];
}
export interface Comparison { slug: string; name: string; parts: { slug: string; quantity: number | null }[] }
export interface PublicData { stamp: string; platform: string; items: PublicItem[]; sets: Comparison[] }
const positive = (n: unknown): number | null => typeof n === 'number' && Number.isFinite(n) && n > 0 ? n : null;
export function publicData(market: Market, slugs?: string[]): PublicData {
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
