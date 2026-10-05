// Watchlist panel: search → pick → add goes through the desktop command with
// the picked slug + threshold; remove and check-now round-trip too.
import { describe, it, expect, vi, afterEach } from 'vitest';
import { screen, fireEvent, waitFor, cleanup } from '@testing-library/svelte';
import { renderDesktop as render } from '../../dev/render-desktop';
import WatchlistPanel from './WatchlistPanel.svelte';
import { installTauri, removeTauri } from '../../dev/test-utils';
import type { Market } from '../../contracts/data';
import { WATCH_FIRED_EVENT } from '../../contracts/events';

afterEach(() => { cleanup(); removeTauri(); vi.restoreAllMocks(); });

const market = {
  updated_at: '2026-08-16T12:00:00Z', platform: 'pc', item_count: 1, catalog_count: 1,
  catalog: { 'primed flow': 'primed_flow' },
  items: { primed_flow: { avg: 20, low_sell: 18, top_buy: 15, vol: 50, ratio: 1, median_now: 20, median_90d: 22 } },
} as unknown as Market;

function makeInvoke(store: { watches: unknown[] }) {
  return vi.fn(async (cmd: string, args?: Record<string, unknown>) => {
    if (cmd === 'list_watches') return store.watches;
    if (cmd === 'add_watch') {
      const w = args!.watch as Record<string, unknown>;
      store.watches = [{ id: store.watches.length + 1, ...w, created_at: 'now', last_price: null, last_checked_at: null, last_fired_at: null }, ...store.watches];
      return store.watches;
    }
    if (cmd === 'delete_watch') { store.watches = store.watches.filter((w) => (w as { id: number }).id !== args!.id); return store.watches; }
    if (cmd === 'check_watches_now') return store.watches.map((w) => ({ ...(w as object), price: 12, satisfied: true, fire: false }));
    throw new Error(`unexpected ${cmd}`);
  });
}

describe('WatchlistPanel', () => {
  it.each(['success', 'failure'])('ignores an older load %s after a watch event', async outcome => {
    let resolve!: (value: unknown[]) => void;
    let reject!: (error: Error) => void;
    const old = new Promise<unknown[]>((yes, no) => { resolve = yes; reject = no; });
    const handlers: Record<string, (event: { payload: unknown }) => void> = {};
    const watch = { id: 1, slug: 'primed_flow', name: 'Primed Flow', side: 'sell', threshold: 15, rank: 0, subtype: null,
      created_at: 'now', last_price: null, last_checked_at: null, last_fired_at: null };
    let loads = 0;
    installTauri(vi.fn(async (cmd: string) => {
      if (cmd === 'list_watches') return ++loads === 1 ? old : [watch];
      throw new Error(`unexpected ${cmd}`);
    }), vi.fn((name, handler) => { handlers[name] = handler; return Promise.resolve(() => {}); }));
    render(WatchlistPanel, { props: { market } });
    await waitFor(() => expect(loads).toBe(1));
    handlers[WATCH_FIRED_EVENT]({ payload: { id: 1, slug: 'primed_flow', name: 'Primed Flow', side: 'sell', threshold: 15, price: 20, satisfied: true, fire: true } });
    await screen.findByRole('button', { name: /^Remove / });
    if (outcome === 'success') resolve([]);
    else reject(new Error('Stale watch failure'));
    await new Promise(done => setTimeout(done, 0));
    expect(screen.getByRole('button', { name: /^Remove / })).toBeTruthy();
    expect(screen.queryByText(/Stale watch failure/)).toBeNull();
  });

  it('unregisters its event listener across unmount and remount', async () => {
    const store = { watches: [] as unknown[] };
    const unlistenFirst = vi.fn();
    const unlistenSecond = vi.fn();
    const listen = vi.fn()
      .mockResolvedValueOnce(unlistenFirst)
      .mockResolvedValueOnce(unlistenSecond);
    installTauri(makeInvoke(store), listen);

    const first = render(WatchlistPanel, { props: { market } });
    await waitFor(() => expect(listen).toHaveBeenCalledTimes(1));
    first.unmount();
    await waitFor(() => expect(unlistenFirst).toHaveBeenCalledTimes(1));

    const second = render(WatchlistPanel, { props: { market } });
    await waitFor(() => expect(listen).toHaveBeenCalledTimes(2));
    second.unmount();
    await waitFor(() => expect(unlistenSecond).toHaveBeenCalledTimes(1));
  });

  it('adds a watch for the picked item at the chosen threshold, then removes it', async () => {
    const store = { watches: [] as unknown[] };
    const invoke = makeInvoke(store);
    installTauri(invoke, undefined);
    render(WatchlistPanel, { props: { market } });
    await screen.findByText('No watches yet. Pick an item above.');

    await fireEvent.input(screen.getByLabelText('Item to watch'), { target: { value: 'primed' } });
    await fireEvent.click(await screen.findByRole('button', { name: /Primed Flow/ }));
    // default threshold for a 'sell' watch = 80% of avg 20 = 16
    expect((screen.getByLabelText('Target price (p)') as HTMLInputElement).value).toBe('16');
    await fireEvent.input(screen.getByLabelText('Target price (p)'), { target: { value: '15' } });
    await fireEvent.click(screen.getByRole('button', { name: 'Add watch' }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('add_watch', {
      watch: { slug: 'primed_flow', name: 'Primed Flow', side: 'sell', threshold: 15, rank: 0, subtype: null },
    }));
    await screen.findByText('ask ≤ 15p');

    await fireEvent.click(screen.getByRole('button', { name: /^Remove / }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('delete_watch', { id: 1 }));
    await screen.findByText('No watches yet. Pick an item above.');
  });

  it('check now runs a pass and reports satisfied watches', async () => {
    const store = { watches: [{ id: 7, slug: 'primed_flow', name: 'Primed Flow', subtype: null, rank: 0, side: 'sell', threshold: 15, created_at: 'x', last_price: null, last_checked_at: null, last_fired_at: null }] };
    const invoke = makeInvoke(store);
    installTauri(invoke, undefined);
    render(WatchlistPanel, { props: { market } });
    await screen.findByText('ask ≤ 15p');
    await fireEvent.click(screen.getByRole('button', { name: 'Check now' }));
    await waitFor(() => expect(invoke.mock.calls.some((c) => c[0] === 'check_watches_now')).toBe(true));
    await screen.findByText('1 watch satisfied right now.');
  });
});

it.each(['manual', 'destroy'])('cancels a watch toast timer on %s dismissal', async dismissal => {
  const store = { watches: [] as unknown[] };
  installTauri(makeInvoke(store), undefined);
  const view = render(WatchlistPanel, { props: { market } });
  await screen.findByText('No watches yet. Pick an item above.');
  const scheduled = vi.spyOn(window, 'setTimeout');
  const cancelled = vi.spyOn(window, 'clearTimeout');
  await fireEvent.click(screen.getByRole('button', { name: 'Check now' }));
  await screen.findByText('No watch is satisfied right now.');
  const index = scheduled.mock.calls.findIndex(call => call[1] === 4500);
  expect(index).toBeGreaterThanOrEqual(0);
  const timer = scheduled.mock.results[index].value;
  if (dismissal === 'manual') await fireEvent.click(screen.getByRole('button', { name: 'Dismiss' }));
  else view.unmount();
  expect(cancelled).toHaveBeenCalledWith(timer);
});
