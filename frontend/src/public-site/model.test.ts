import { describe, it, expect } from 'vitest';
import { activityCandidates, datedHistory, decisionAnswer, decisionText, ducatParts, formatStampUtc, formatStampUtcTime, publicData, partTotal, priceText } from './model';
import type { Market, PublicMarket } from '../contracts/data';
import bundled from '../../public/market.json';
import publicFields from '../../../tests/fixtures/public-market/fields.json';

const market = {
  updated_at: '2026-10-20T12:00:00Z', platform: 'pc',
  catalog: { 'bo prime set':'bo_prime_set', 'bo prime ornament':'bo_prime_ornament' },
  items: { bo_prime_set: { price_basis:'unit', median_now:30, vol:0 }, bo_prime_ornament: { price_basis:'unit', median_now:7, ducats:15, medians_7d:[7,8] } },
  set_to_parts: { bo_prime_set: { name:'Bo Prime', parts:[{ slug:'bo_prime_ornament', quantity:2 }] } },
} as unknown as Market;

// The site's pages refresh from market-public.json, which the scraper writes
// with only the fields listed in the fixture (and catalog names of priced
// items). On the real snapshot they must read the same answer from it.
describe('market-public.json', () => {
  it('gives the prerendered pages the same data as the full snapshot', () => {
    const full = bundled as unknown as Market;
    const pick = (source: object, keys: string[]) => Object.fromEntries(keys.filter(key => key in source).map(key => [key, (source as Record<string, unknown>)[key]]));
    const top = pick(full, publicFields.snapshot) as Partial<Market>;
    if (top.catalog) top.catalog = Object.fromEntries(Object.entries(top.catalog).filter(([, slug]) => slug in full.items));
    const slim = {
      ...top,
      items: Object.fromEntries(Object.entries(full.items).map(([slug, item]) => [slug, pick(item, publicFields.item)])),
    } as unknown as PublicMarket;
    expect(publicData(slim)).toEqual(publicData(full));
  });
});

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


describe('public set-versus-parts decision', () => {
  const data = () => publicData(market);
  it('uses a quantity-weighted sum and states one set versus copies and distinct items', () => {
    const answer = decisionAnswer(data(), 'bo_prime_set');
    expect(answer).not.toBeNull();
    expect(answer).toMatchObject({ outcome: 'set-higher', setPrice: 30, partsTotal: 14, gap: 16, componentCopies: 2, distinctComponents: 1, setVolume48h: 0 });
    const text = decisionText(answer!);
    expect(text.headline).toBe('The set is 16p above the summed parts.');
    expect(text.quantities).toBe('One set versus 2 component copies across 1 distinct item.');
    expect(text.clipboard).toContain('set 30p vs 14p summed parts');
    expect(text.clipboard).toContain('https://tennoworth.app/items/bo_prime_set/');
    expect(formatStampUtc('2026-10-20T12:00:00Z')).toBe('20 Oct 2026');
    expect(formatStampUtcTime('2026-10-20T12:00:00Z')).toBe('20 Oct 2026 12:00 UTC');
    expect(priceText(30)).toBe('30p');
    expect(priceText(null)).toBe('Unavailable');
  });
  it('covers parts-higher, tie and missing/zero volume without claiming sale speed', () => {
    const higher = structuredClone(market); higher.items.bo_prime_set.median_now = 5;
    expect(decisionAnswer(publicData(higher), 'bo_prime_set')).toMatchObject({ outcome: 'parts-higher', gap: -9 });
    const tie = structuredClone(market); tie.items.bo_prime_set.median_now = 14;
    const tieAnswer = decisionAnswer(publicData(tie), 'bo_prime_set')!;
    expect(tieAnswer.outcome).toBe('tie');
    expect(decisionText(tieAnswer).headline).toBe('The set and summed parts are equal at 14p.');
    const noVolume = structuredClone(market); delete (noVolume.items.bo_prime_set as unknown as { vol?: number }).vol;
    expect(decisionText(decisionAnswer(publicData(noVolume), 'bo_prime_set')!).evidence).toContain('Set reported volume: unavailable.');
    expect(decisionText(tieAnswer).evidence).toContain('Source: warframe.market');
    expect(decisionText(tieAnswer).evidence).toContain('does not establish which sells faster');
    expect(decisionText(tieAnswer).clipboard).toContain('not evidence of sale speed');
  });
  it('suppresses the verdict when a component price, quantity or platform is not comparable', () => {
    const noPrice = structuredClone(market); delete noPrice.items.bo_prime_ornament.median_now;
    const missingPrice = decisionAnswer(publicData(noPrice), 'bo_prime_set')!;
    expect(missingPrice).toMatchObject({ outcome: 'unavailable', reason: 'missing-data', partsTotal: null, gap: null });
    expect(decisionText(missingPrice).clipboard).toContain('comparison unavailable');
    const noQuantity = structuredClone(market); delete noQuantity.set_to_parts!.bo_prime_set.parts[0].quantity;
    const missingQuantity = decisionAnswer(publicData(noQuantity), 'bo_prime_set')!;
    expect(missingQuantity).toMatchObject({ outcome: 'unavailable', reason: 'missing-data', componentCopies: null });
    const console = structuredClone(market); console.platform = 'ps4';
    expect(decisionAnswer(publicData(console), 'bo_prime_set')).toMatchObject({ outcome: 'unavailable', reason: 'incomparable-data' });
    const badStamp = structuredClone(market); badStamp.updated_at = 'not-a-date';
    expect(decisionAnswer(publicData(badStamp), 'bo_prime_set')).toMatchObject({ outcome: 'unavailable', reason: 'invalid-snapshot' });
  });
  it('rejects duplicate component mappings, nonfinite totals and a missing mapping without dropping the answer', () => {
    const duplicate = structuredClone(market);
    duplicate.set_to_parts!.bo_prime_set.parts.push({ slug: 'bo_prime_ornament', component_name: 'Bo Prime Ornament', quantity: 2 });
    const duplicateAnswer = decisionAnswer(publicData(duplicate), 'bo_prime_set')!;
    expect(duplicateAnswer).toMatchObject({ outcome: 'unavailable', reason: 'incomparable-data' });
    expect(duplicateAnswer.missing.join(' ')).toContain('duplicate component mapping');
    const overflow = structuredClone(market); overflow.items.bo_prime_ornament.median_now = Number.MAX_VALUE;
    const overflowAnswer = decisionAnswer(publicData(overflow), 'bo_prime_set')!;
    expect(overflowAnswer).toMatchObject({ outcome: 'unavailable', reason: 'incomparable-data', partsTotal: null });
    const noMapping = structuredClone(market); delete noMapping.set_to_parts;
    const noMappingAnswer = decisionAnswer(publicData(noMapping), 'bo_prime_set')!;
    expect(noMappingAnswer).toMatchObject({ outcome: 'unavailable', reason: 'missing-data' });
    expect(noMappingAnswer.setOrdersUrl).toContain('warframe.market/items/bo_prime_set');
  });
  it('is absent outside the pilot sets and preserves unknown volume', () => {
    expect(decisionAnswer(data(), 'not_a_pilot_set')).toBeNull();
    expect(decisionAnswer(data(), 'bo_prime_blueprint')).toBeNull();
    const noVolume = structuredClone(market); delete (noVolume.items.bo_prime_set as unknown as { vol?: number }).vol;
    expect(decisionAnswer(publicData(noVolume), 'bo_prime_set')?.setVolume48h).toBeNull();
  });
});
