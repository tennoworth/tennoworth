import type { Market } from '../contracts/data';
import { buildBrowseIndex } from '../domain/market-browse';
import { pilotSets } from './pages';

export interface PublicItem {
  slug: string; name: string; price: number | null; volume: number | null;
  ducats: number | null; baseline: number | null; recent: number[];
}
export interface Comparison { slug: string; name: string; parts: { slug: string; quantity: number | null }[] }
export interface PublicData { stamp: string; platform: string; items: PublicItem[]; sets: Comparison[] }
const positive = (n: unknown): number | null => typeof n === 'number' && Number.isFinite(n) && n > 0 ? n : null;
export function publicData(market: Market, slugs?: string[]): PublicData {
  const names = buildBrowseIndex(market);
  const selected = slugs ? new Set(slugs) : null;
  return {
    stamp: market.updated_at, platform: market.platform,
    items: Object.entries(market.items).filter(([slug]) => !selected || selected.has(slug)).map(([slug, item]) => ({
      slug, name: names.nameOf(slug), price: item.price_basis === 'unit' ? positive(item.median_now) : null,
      volume: typeof item.vol === 'number' && Number.isFinite(item.vol) && item.vol >= 0 ? item.vol : null,
      ducats: positive(item.ducats), baseline: item.price_basis === 'unit' ? positive(item.median_90d) : null,
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
