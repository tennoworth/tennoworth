import { describe, it, expect } from 'vitest';
import { publicData, partTotal } from './model';
import type { Market } from '../contracts/data';

const market = {
  updated_at: '2026-10-20T12:00:00Z', platform: 'pc',
  catalog: { 'bo prime set':'bo_prime_set', 'bo prime ornament':'bo_prime_ornament' },
  items: { bo_prime_set: { price_basis:'unit', median_now:30, vol:0 }, bo_prime_ornament: { price_basis:'unit', median_now:7, ducats:15, medians_7d:[7,8] } },
  set_to_parts: { bo_prime_set: { name:'Bo Prime', parts:[{ slug:'bo_prime_ornament', quantity:2 }] } },
} as unknown as Market;

describe('public selling comparisons', () => {
  it('compares required component quantities and preserves genuine zero activity', () => {
    const data = publicData(market);
    expect(partTotal(data.sets[0], data)).toBe(14);
    expect(data.items[0].volume).toBe(0);
  });
  it('refuses a complete total when a required price or quantity is unknown', () => {
    const noPrice = structuredClone(market); delete noPrice.items.bo_prime_ornament.median_now;
    const data = publicData(noPrice);
    expect(partTotal(data.sets[0], data)).toBeNull();
    const noQuantity = structuredClone(market); delete noQuantity.set_to_parts!.bo_prime_set.parts[0].quantity;
    const missing = publicData(noQuantity);
    expect(partTotal(missing.sets[0], missing)).toBeNull();
  });
  it('does not imply unit prices when the price basis is unknown', () => {
    const unknown = structuredClone(market); delete unknown.items.bo_prime_set.price_basis;
    expect(publicData(unknown).items[0].price).toBeNull();
  });
  it('keeps invalid prices unavailable and projects only the requested items', () => {
    const invalid = structuredClone(market); invalid.items.bo_prime_set.median_now = NaN;
    expect(publicData(invalid, ['bo_prime_set']).items).toMatchObject([{ price:null, volume:0, ducats:null }]);
    expect(publicData(invalid, ['bo_prime_set']).items).toHaveLength(1);
  });
});
