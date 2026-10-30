// Market browser: the 1-year toggle loads history on demand (never at mount),
// swaps the row's 7-day trend for the year view, and degrades honestly when
// history is unavailable or the item's history is thin.
import { describe, it, expect, vi, afterEach } from 'vitest';
import { screen, fireEvent, waitFor, cleanup } from '@testing-library/svelte';
import { renderDesktop as render } from '../../dev/preview/render-desktop';
import MarketBrowser from './MarketBrowser.svelte';
import type { Market } from '../../contracts/data';
import type { History } from '../../domain/history';

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

it('hosts Meta Drift below the market reports when annual usage is present', () => {
  const withUsage = {
    ...market,
    usage_history: {
      years: [2023, 2025],
      by_year: {
        2023: { primed_flow: { name: 'Primed Flow', category: 'Mods', share: 1 } },
        2025: { primed_flow: { name: 'Primed Flow', category: 'Mods', share: 2 } },
      },
    },
  } as unknown as Market;
  render(MarketBrowser, { props: { market: withUsage } });
  expect(screen.getByTestId('meta-drift')).toBeTruthy();
  expect(screen.getByText('+1.00 pp')).toBeTruthy();
});

// Hosted search lives in `?q=` so Back from an item page restores it. It must
// rewrite the current history entry, never add one per keystroke.
describe('MarketBrowser search URL', () => {
  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
    window.history.replaceState(null, '', '/');
  });
  const input = () => screen.getByLabelText('Search items') as HTMLInputElement;

  it('opens on the query in the URL', async () => {
    window.history.replaceState(null, '', '/?q=primed');
    render(MarketBrowser, { props: { market, syncUrl: true } });
    expect(input().value).toBe('primed');
    expect(screen.getByText(/1 match/)).toBeTruthy();
  });

  it('replaces the current entry once typing settles, keeping other params and the hash', async () => {
    vi.useFakeTimers();
    window.history.replaceState(null, '', '/?ref=x#faq');
    const length = window.history.length;
    const replace = vi.spyOn(window.history, 'replaceState');
    const push = vi.spyOn(window.history, 'pushState');
    render(MarketBrowser, { props: { market, syncUrl: true } });
    for (const value of ['b', 'bo', 'bo prime']) await fireEvent.input(input(), { target: { value } });
    expect(replace).not.toHaveBeenCalled();
    vi.advanceTimersByTime(250);
    expect(replace).toHaveBeenCalledTimes(1);
    expect(`${location.pathname}${location.search}${location.hash}`).toBe('/?ref=x&q=bo+prime#faq');
    expect(push).not.toHaveBeenCalled();
    expect(window.history.length).toBe(length);
  });

  it('writes a pending query on blur so a clicked result never outruns it', async () => {
    vi.useFakeTimers();
    render(MarketBrowser, { props: { market, syncUrl: true } });
    await fireEvent.input(input(), { target: { value: 'flow' } });
    await fireEvent.blur(input());
    expect(location.search).toBe('?q=flow');
  });

  it('removes q when the search is cleared', async () => {
    vi.useFakeTimers();
    window.history.replaceState(null, '', '/?q=primed');
    render(MarketBrowser, { props: { market, syncUrl: true } });
    await fireEvent.input(input(), { target: { value: '  ' } });
    vi.advanceTimersByTime(250);
    expect(location.search).toBe('');
  });

  it('leaves the URL alone where search is not URL-backed (the desktop app)', async () => {
    vi.useFakeTimers();
    window.history.replaceState(null, '', '/?q=primed');
    const replace = vi.spyOn(window.history, 'replaceState');
    render(MarketBrowser, { props: { market } });
    expect(input().value).toBe('');
    await fireEvent.input(input(), { target: { value: 'flow' } });
    vi.advanceTimersByTime(250);
    await fireEvent.blur(input());
    expect(replace).not.toHaveBeenCalled();
  });
});
