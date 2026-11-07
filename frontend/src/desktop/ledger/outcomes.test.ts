import { describe, it, expect } from 'vitest';
import type { ListingOutcomes, OutcomeGroup } from '../../contracts/desktop';
import { outcomeRows, outcomeHeadline } from './outcomes';

describe('listing outcomes', () => {
  const none: OutcomeGroup = { listings: 0, quantity_listed: 0, quantity_sold: 0, within_day: { eligible: 0, sold: 0 }, within_week: { eligible: 0, sold: 0 }, median_hours_to_first_sale: null, price_ratio: null, priced_quantity: 0 };
  const o = (session: OutcomeGroup, manual: OutcomeGroup = none, enabled = true): ListingOutcomes => ({ enabled, window_days: 30, session, manual });

  it('shows unknowns as a dash, never as zero', () => {
    const [row] = outcomeRows(o({ ...none, listings: 2, quantity_listed: 2 }));
    expect(row).toMatchObject({ label: 'Trade Session', listings: 2, wait: '-', price: '-' });
    expect(row.meters.map((m) => m.text)).toEqual(['0 of 2', 'too recent', 'too recent']);
  });

  it('formats waits in hours, then days', () => {
    const wait = (h: number) => outcomeRows(o({ ...none, listings: 1, median_hours_to_first_sale: h }))[0].wait;
    expect([wait(0.4), wait(5.2), wait(47.6), wait(61)]).toEqual(['under 1 h', '5 h', '48 h', '2.5 d']);
  });

  it('says when nothing is old enough to judge, or when it is off', () => {
    expect(outcomeHeadline(o({ ...none, listings: 1 }))).toBe('1 listing in 30 days · too recent to judge');
    expect(outcomeHeadline(o(none))).toBe('none in the last 30 days');
    expect(outcomeHeadline(o(none, none, false))).toBe('off');
  });

  it('meters a share only once something is old enough to judge', () => {
    const [row] = outcomeRows(o({ ...none, listings: 3, quantity_listed: 5, quantity_sold: 2, within_day: { eligible: 0, sold: 0 }, within_week: { eligible: 2, sold: 1 } }));
    expect(row.meters).toEqual([
      { label: 'Copies sold', value: 2, max: 5, text: '2 of 5' },
      { label: 'Sold in a day', value: 0, max: 0, text: 'too recent' },
      { label: 'Sold in a week', value: 1, max: 2, text: '1 of 2' },
    ]);
  });
});

