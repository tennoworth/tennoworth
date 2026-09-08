import { afterEach, expect, it, vi } from 'vitest';
import { cleanup, render, screen } from '@testing-library/svelte';
import { DESKTOP_CONTEXT } from '../../contracts/services';
import type { OwnedRecord } from '../../contracts/data';
import BaroBoard from './BaroBoard.svelte';
afterEach(cleanup);
it('keeps the stock surface usable without inventory and does not invoke planning', async () => {
  const ducatPlan = vi.fn();
  render(BaroBoard, { props: { market: null, baro: null }, context: new Map([[DESKTOP_CONTEXT, { ducatPlan }]]) });
  await screen.findByText('What he is selling');
  expect(ducatPlan).not.toHaveBeenCalled();
  expect(screen.queryByRole('alert')).toBeNull();
});
it('does not present failed scrap planning as zero ducat potential', async () => {
  const ducatPlan = vi.fn().mockRejectedValue(new Error('native unavailable'));
  const owned = new Map([['a', { slug: 'a', name: 'A', count: 3 } as OwnedRecord]]);
  render(BaroBoard, { props: { market: null, baro: null, owned }, context: new Map([[DESKTOP_CONTEXT, { ducatPlan }]]) });
  expect((await screen.findByRole('alert')).textContent).toContain('Scrap planning unavailable');
  expect(screen.queryByText(/Scrapping every spare/)).toBeNull();
});
