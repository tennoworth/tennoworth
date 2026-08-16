// Watchlist panel: search → pick → add goes through the desktop command with
// the picked slug + threshold; remove and check-now round-trip too.
import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, screen, fireEvent, waitFor, cleanup } from '@testing-library/svelte';
import WatchlistPanel from './WatchlistPanel.svelte';
import { installTauri, removeTauri } from '../lib/test-utils';
import type { Market } from '../lib/types';

afterEach(() => { cleanup(); removeTauri(); });

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
  it('adds a watch for the picked item at the chosen threshold, then removes it', async () => {
    const store = { watches: [] as unknown[] };
    const invoke = makeInvoke(store);
    installTauri(invoke, undefined);
    render(WatchlistPanel, { props: { market } });
    await screen.findByText('No watches yet. Pick an item above.');

    await fireEvent.input(screen.getByLabelText('Item to watch'), { target: { value: 'primed' } });
    await fireEvent.click(await screen.findByRole('button', { name: /Primed Flow/ }));
    // default threshold for a 'sell' watch = 80% of avg 20 = 16
    expect((screen.getByLabelText('Threshold (plat)') as HTMLInputElement).value).toBe('16');
    await fireEvent.input(screen.getByLabelText('Threshold (plat)'), { target: { value: '15' } });
    await fireEvent.click(screen.getByRole('button', { name: 'Add watch' }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('add_watch', {
      watch: { slug: 'primed_flow', name: 'Primed Flow', side: 'sell', threshold: 15, rank: 0, subtype: null },
    }));
    await screen.findByText('ask ≤ 15p');

    await fireEvent.click(screen.getByRole('button', { name: 'Remove' }));
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
