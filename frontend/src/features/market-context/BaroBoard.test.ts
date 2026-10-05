import { afterEach, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen, within } from '@testing-library/svelte';
import { DESKTOP_CONTEXT } from '../../contracts/services';
import type { Market, OwnedRecord } from '../../contracts/data';
import BaroBoard from './BaroBoard.svelte';
afterEach(cleanup);
const emptyValue = { rows: [], best: [], moreTradeable: 0, fodderDucats: null, fodderItems: null, cheapFodder: null };
const baroValue = vi.fn().mockResolvedValue(emptyValue);
it('keeps the stock surface usable without inventory and does not invoke planning', async () => {
  const ducatPlan = vi.fn();
  render(BaroBoard, { props: { market: null, baro: null }, context: new Map([[DESKTOP_CONTEXT, { ducatPlan, baroValue }]]) });
  await screen.findByText('What he is selling');
  expect(ducatPlan).not.toHaveBeenCalled();
  expect(screen.queryByRole('alert')).toBeNull();
});
it('does not present failed scrap planning as zero ducat potential', async () => {
  const ducatPlan = vi.fn().mockRejectedValue(new Error('native unavailable'));
  const owned = new Map([['a', { slug: 'a', name: 'A', count: 3 } as OwnedRecord]]);
  render(BaroBoard, { props: { market: null, baro: null, owned }, context: new Map([[DESKTOP_CONTEXT, { ducatPlan, baroValue }]]) });
  expect((await screen.findByRole('alert')).textContent).toContain('Scrap planning unavailable');
  expect(screen.queryByText(/Scrapping every spare/)).toBeNull();
});

it('keeps plain rounded prices with a p suffix and missing prices without a suffix', async () => {
  const market = { items: {
    primed_flow: { low_sell: 2499.6, median_90d: 1999.6, vol: 40 },
  } } as unknown as Market;
  const baro = {
    activation: '2026-10-02T00:00:00Z', expiry: '2026-10-04T00:00:00Z', location: 'Relay',
    inventory: [
      { item: 'Primed Flow', slug: 'primed_flow', ducats: 350 },
      { item: 'Cosmetic', ducats: 200 },
    ],
  };
  render(BaroBoard, { props: { market, baro }, context: new Map([[DESKTOP_CONTEXT, { ducatPlan: vi.fn(), baroValue: vi.fn().mockResolvedValue({ ...emptyValue, rows: [
    { ...baro.inventory[0], price: 2499.6, baseline: 1999.6, vol: 40, platPerDucat: 2499.6 / 350, verdict: 'flip' },
    { ...baro.inventory[1], price: null, baseline: null, vol: null, platPerDucat: null, verdict: 'unpriced' },
  ] }) }]]) });
  const priced = await screen.findByRole('row', { name: /Primed Flow/ });
  expect(within(priced).getByText('2500p')).toBeTruthy();
  expect(within(priced).getByText('2000p')).toBeTruthy();
  await fireEvent.click(screen.getByRole('button', { name: /Show 1 more/ }));
  const unpriced = screen.getByRole('row', { name: /Cosmetic/ });
  expect(within(unpriced).getAllByText('-').length).toBeGreaterThanOrEqual(2);
  expect(unpriced.textContent).not.toContain('-p');
});

it('shows failed pricing without claiming an empty stock', async () => {
  render(BaroBoard, { props: { market: null, baro: null }, context: new Map([[DESKTOP_CONTEXT, { ducatPlan: vi.fn(), baroValue: vi.fn().mockRejectedValue(new Error('unavailable')) }]]) });
  expect((await screen.findByRole('alert')).textContent).toContain('Stock pricing unavailable');
  expect(screen.queryByText(/No items in the current value filter/)).toBeNull();
});
