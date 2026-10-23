import { describe, it, expect } from 'vitest';
import { activityCandidates, datedHistory, ducatParts, publicData, partTotal } from './model';
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

describe('public discovery lists', () => {
  it('includes evidenced individual Prime parts and never treats a set total as kiosk eligibility', () => {
    const m = structuredClone(market);
    m.items.bo_prime_set.tags = ['prime', 'set']; m.items.bo_prime_set.ducats = 90;
    m.items.bo_prime_ornament.tags = ['prime'];
    const data = publicData(m);
    expect(ducatParts(data).map(item => item.slug)).toEqual(['bo_prime_ornament']);
    delete m.set_to_parts;
    expect(ducatParts(publicData(m))).toEqual([]);
    m.items.bo_prime_ornament.tags = ['prime', 'component'];
    expect(ducatParts(publicData(m)).map(item => item.slug)).toEqual(['bo_prime_ornament']);
  });
  it('requires a positive known unit price for the ducat comparison', () => {
    const m = structuredClone(market); m.items.bo_prime_ornament.tags = ['prime','component'];
    m.items.bo_prime_ornament.median_now = 0;
    expect(ducatParts(publicData(m))).toEqual([]);
  });
  it('orders candidates by observed activity, excluding unknowns instead of fabricating a score', () => {
    const m = structuredClone(market);
    m.items.bo_prime_set.tags = ['prime','set']; m.items.bo_prime_set.vol = 50;
    m.items.bo_prime_ornament.tags = ['prime','component']; m.items.bo_prime_ornament.vol = 100;
    expect(activityCandidates(publicData(m)).map(item => item.slug)).toEqual(['bo_prime_ornament','bo_prime_set']);
    m.items.bo_prime_ornament.median_now = NaN;
    expect(activityCandidates(publicData(m)).map(item => item.slug)).toEqual(['bo_prime_set']);
    m.items.bo_prime_set.vol = 0;
    expect(activityCandidates(publicData(m))).toEqual([]);
  });
});

const dated = { start:'2025-12-30', through:'2026-01-02', days:4, generated_at:'2026-01-03T00:00:00Z',
  missing_days:['2025-12-31'], items:{ bo_prime_set:{ median:[5,6,null,9], volume:[1,2,0,3] } } };
describe('dated item history', () => {
  it('aligns exact UTC dates across a year boundary and retains source gaps', () => {
    expect(datedHistory(dated,'bo_prime_set')?.rows).toEqual([
      {date:'2026-01-02',median:9,volume:3}, {date:'2026-01-01',median:null,volume:0},
      {date:'2025-12-31',median:null,volume:null}, {date:'2025-12-30',median:5,volume:1},
    ]);
  });
  it('refuses malformed dates, mismatched arrays and incompatible subtypes', () => {
    for (const change of [{ start:'2025-02-31' }, { through:'2026-99-31' }, { days:0 }, { days:4001 }, { items:{bo_prime_set:{median:[1],volume:[]}} }, { items:{bo_prime_set:{...dated.items.bo_prime_set,subtype:'rank5'}} }]) {
      expect(datedHistory({...dated,...change},'bo_prime_set')).toBeNull();
    }
    expect(datedHistory(null,'bo_prime_set')).toBeNull();
    expect(datedHistory(dated,'unknown')).toBeNull();
  });
  it('does not turn invalid observations into plausible prices or carry data beyond through', () => {
    const h = structuredClone(dated); h.through='2025-12-31'; h.items.bo_prime_set.median[0]=NaN;
    expect(datedHistory(h,'bo_prime_set')?.rows).toHaveLength(2);
    expect(datedHistory(h,'bo_prime_set')?.rows[1].median).toBeNull();
  });
});
