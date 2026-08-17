// Market browser: the 1-year toggle loads history on demand (never at mount),
// swaps the row's 7-day trend for the year view, and degrades honestly when
// history is unavailable or the item's history is thin.
import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, screen, fireEvent, waitFor, cleanup } from '@testing-library/svelte';
import MarketBrowser from './MarketBrowser.svelte';
import type { Market } from '../lib/types';
import type { History } from '../lib/history';

afterEach(cleanup);

const market = {
  updated_at: '2026-08-16T12:00:00Z', platform: 'pc', item_count: 2, catalog_count: 2,
  catalog: { 'primed flow': 'primed_flow', 'thin thing': 'thin_thing' },
  items: {
    primed_flow: { avg: 20, low_sell: 18, top_buy: 15, vol: 50, ratio: 1, median_now: 20, median_90d: 22, medians_7d: [20, 21, 20, 22, 20, 21, 20] },
    thin_thing: { avg: 5, low_sell: 5, top_buy: 4, vol: 30, ratio: 1, median_now: 5, median_90d: 5, medians_7d: [5, 5, 5, 5, 5, 5, 5] },
  },
} as unknown as Market;

const history: History = {
  generated_at: 'g', start: '2025-08-17', days: 40, through: '2026-08-15',
  items: {
    // 30 days at 10, then 10 days at 20 → Δ1y +100%
    primed_flow: { median: [...Array(30).fill(10), ...Array(10).fill(20)], volume: Array(40).fill(5) },
    thin_thing: { median: [5, null, 5, ...Array(37).fill(null)], volume: Array(40).fill(0) },
  },
};

describe('MarketBrowser 1-year view', () => {
  it('does not load history at mount; loads on toggle and shows Δ1y', async () => {
    const loadHistory = vi.fn().mockResolvedValue(history);
    render(MarketBrowser, { props: { market, loadHistory } });
    expect(loadHistory).not.toHaveBeenCalled();
    await fireEvent.input(screen.getByLabelText('Search items'), { target: { value: 'primed' } });
    await screen.findAllByText('Primed Flow'); // search row + top-movers/vaulted lists may repeat it
    await fireEvent.click(screen.getByRole('button', { name: '1 year' }));
    await waitFor(() => expect(loadHistory).toHaveBeenCalledTimes(1));
    await screen.findAllByText('▲100% 1y');
    expect(screen.getByText(/through 2026-08-15/)).toBeTruthy();
    // toggling back does not refetch
    await fireEvent.click(screen.getByRole('button', { name: '1 year' }));
    await fireEvent.click(screen.getByRole('button', { name: '1 year' }));
    expect(loadHistory).toHaveBeenCalledTimes(1);
  });

  it('says thin history for items without enough traded days', async () => {
    render(MarketBrowser, { props: { market, loadHistory: vi.fn().mockResolvedValue(history) } });
    await fireEvent.input(screen.getByLabelText('Search items'), { target: { value: 'thin' } });
    await screen.findAllByText('Thin Thing');
    await fireEvent.click(screen.getByRole('button', { name: '1 year' }));
    await screen.findAllByText('thin history');
  });

  it('reports unavailable when the loader returns null, and hides the toggle without a loader', async () => {
    const { unmount } = render(MarketBrowser, { props: { market, loadHistory: vi.fn().mockResolvedValue(null) } });
    await fireEvent.click(screen.getByRole('button', { name: '1 year' }));
    await screen.findByText(/history unavailable/);
    unmount();
    render(MarketBrowser, { props: { market } });
    expect(screen.queryByRole('button', { name: '1 year' })).toBeNull();
  });
});
