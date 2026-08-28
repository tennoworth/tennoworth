// Listing-health wiring for the My orders panel: the "not owned" check from
// the scan, the desktop "Check live" round-trip, and that a fix goes through
// the same transport call as a manual edit.
import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, screen, fireEvent, waitFor, cleanup } from '@testing-library/svelte';
import MyOrdersPanel from './MyOrdersPanel.svelte';
import type { Transport } from '../lib/transport';
import { installTauri, removeTauri } from '../lib/test-utils';

afterEach(() => { cleanup(); removeTauri(); });

const ORDERS = {
  data: {
    sell: [
      { id: 'o1', platinum: 20, visible: true, quantity: 1, rank: 0, item: { name: 'Primed Flow', slug: 'primed_flow' } },
      { id: 'o2', platinum: 30, visible: true, quantity: 3, item: { name: 'Ash Prime Blueprint', slug: 'ash_prime_blueprint' } },
    ],
    buy: [],
  },
};

function makeTransport(overrides: Partial<Transport> = {}) {
  return {
    fetchOrders: vi.fn().mockResolvedValue(ORDERS),
    updateOrder: vi.fn().mockResolvedValue({ status: 'ok' }),
    deleteOrder: vi.fn().mockResolvedValue(undefined),
    bulkVisibility: vi.fn().mockResolvedValue({ results: [] }),
    ...overrides,
  } as unknown as Transport;
}

describe('MyOrdersPanel listing health', () => {
  it('unregisters a lazily armed live-progress listener on unmount', async () => {
    const unlisten = vi.fn();
    const listen = vi.fn().mockResolvedValue(unlisten);
    const invoke = vi.fn().mockResolvedValue([]);
    installTauri(invoke, listen);
    const panel = render(MyOrdersPanel, { props: { transport: makeTransport() } });
    await screen.findByText('Primed Flow');
    await fireEvent.click(screen.getByRole('button', { name: 'Check live' }));
    await waitFor(() => expect(listen).toHaveBeenCalledTimes(1));
    panel.unmount();
    await waitFor(() => expect(unlisten).toHaveBeenCalledTimes(1));
  });

  it('flags a listing the last scan says you no longer own, and Delete removes it', async () => {
    const transport = makeTransport();
    const ownedQty = new Map([['primed_flow|', 1], ['ash_prime_blueprint|', 0]]);
    render(MyOrdersPanel, { props: { transport, ownedQty } });
    await screen.findByText('1 not owned');
    await fireEvent.click(screen.getByRole('button', { name: 'Delete' }));
    await waitFor(() => expect(transport.deleteOrder).toHaveBeenCalledWith('o2'));
  });

  it('flags a listing quantity above what you own and Set qty patches it', async () => {
    const transport = makeTransport();
    const ownedQty = new Map([['primed_flow|', 1], ['ash_prime_blueprint|', 2]]);
    render(MyOrdersPanel, { props: { transport, ownedQty } });
    await screen.findByText('1 over-quantity');
    await fireEvent.click(screen.getByRole('button', { name: 'Set qty' }));
    await waitFor(() => expect(transport.updateOrder).toHaveBeenCalledWith('o2', { quantity: 2 }));
  });

  it('desktop: Check live asks for each sell listing\'s tier and Reprice matches the lowest other ask', async () => {
    const invoke = vi.fn(async (cmd: string, args: { queries: Array<{ slug: string; rank: number; subtype: string | null }> }) => {
      expect(cmd).toBe('live_top_prices');
      return args.queries.map((q) => ({
        slug: q.slug, rank: q.rank, subtype: q.subtype,
        sells: q.slug === 'primed_flow' ? [15] : [], buys: [],
        low_sell: q.slug === 'primed_flow' ? 15 : null, top_buy: null,
        own_ask: q.slug === 'primed_flow' ? 20 : null, own_bid: null, error: null,
      }));
    });
    installTauri(invoke, undefined);
    const transport = makeTransport();
    render(MyOrdersPanel, { props: { transport } });
    await screen.findByText('Primed Flow');
    await fireEvent.click(screen.getByRole('button', { name: 'Check live' }));
    await waitFor(() => expect(invoke).toHaveBeenCalledTimes(1));
    expect(invoke.mock.calls[0][1].queries).toEqual([
      { slug: 'primed_flow', rank: 0, subtype: null },
      { slug: 'ash_prime_blueprint', rank: 0, subtype: null },
    ]);
    await screen.findByText('1 above the market');
    await fireEvent.click(screen.getByRole('button', { name: 'Reprice' }));
    await waitFor(() => expect(transport.updateOrder).toHaveBeenCalledWith('o1', { platinum: 15 }));
  });

  it('hosted: no Check live button', async () => {
    render(MyOrdersPanel, { props: { transport: makeTransport() } });
    await screen.findByText('Primed Flow');
    expect(screen.queryByRole('button', { name: /Check live/ })).toBeNull();
  });
});
