// Listing-health wiring for the My orders panel: the "not owned" check from
// the scan, the desktop "Check live" round-trip, and that a fix goes through
// the same transport call as a manual edit.
import { describe, it, expect, vi, afterEach } from 'vitest';
import { screen, fireEvent, waitFor, cleanup } from '@testing-library/svelte';
import { renderDesktop as render } from '../../dev/render-desktop';
import MyOrdersPanel from './MyOrdersPanel.svelte';
import type { DesktopCapabilities } from '../../contracts/desktop';
import type { Market } from '../../contracts/data';
import { installTauri, removeTauri } from '../../dev/test-utils';

afterEach(() => { cleanup(); removeTauri(); });

const ORDERS = [
      { id: 'o1', platinum: 20, visible: true, quantity: 1, rank: 0, name: 'Primed Flow', slug: 'primed_flow', item_id: 'primed_flow', side: 'sell' as const, per_trade: null, subtype: null },
      { id: 'o2', platinum: 30, visible: true, quantity: 3, name: 'Ash Prime Blueprint', slug: 'ash_prime_blueprint', item_id: 'ash_prime_blueprint', side: 'sell' as const, per_trade: null, subtype: null },
  ];

function makeTransport(overrides: Partial<DesktopCapabilities> = {}) {
  return {
    fetchOrders: vi.fn().mockResolvedValue(ORDERS),
    updateOrder: vi.fn().mockResolvedValue({ status: 'ok' }),
    deleteOrder: vi.fn().mockResolvedValue(undefined),
    bulkVisibility: vi.fn().mockResolvedValue({ results: [] }),
    ...overrides,
  } as unknown as DesktopCapabilities;
}

describe('MyOrdersPanel listing health', () => {
  it('keeps an unreported visibility state unknown and blocks its toggle', async () => {
    const transport = makeTransport({ fetchOrders: vi.fn().mockResolvedValue([
      { ...ORDERS[0], visible: null },
    ]) });
    render(MyOrdersPanel, { props: { transport } });
    await screen.findByText('Primed Flow');
    const toggle = screen.getByTitle('Visibility unavailable; refresh orders') as HTMLButtonElement;
    expect(toggle.disabled).toBe(true);
    expect(toggle.textContent).toContain('?');
    expect(screen.getByRole('button', { name: 'Hidden 0' })).toBeTruthy();
    expect(transport.updateOrder).not.toHaveBeenCalled();
  });

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

  it('flags a listing the last scan says you no longer own, and deleting it takes a confirmation', async () => {
    const transport = makeTransport();
    const ownedQty = new Map([['primed_flow|', 1], ['ash_prime_blueprint|', 0]]);
    render(MyOrdersPanel, { props: { transport, ownedQty } });
    await screen.findByText('1 not owned');
    await fireEvent.click(screen.getByRole('button', { name: 'Delete' }));
    // Removing a live listing is consequential: one click arms, it does not act.
    expect(transport.deleteOrder).not.toHaveBeenCalled();
    await fireEvent.click(screen.getByRole('button', { name: 'Confirm' }));
    await waitFor(() => expect(transport.deleteOrder).toHaveBeenCalledWith('o2'));
  });

  it('shows no confirmation until the destructive fix is armed, and cancel restores it', async () => {
    const transport = makeTransport();
    const ownedQty = new Map([['primed_flow|', 1], ['ash_prime_blueprint|', 0]]);
    render(MyOrdersPanel, { props: { transport, ownedQty } });
    await screen.findByText('1 not owned');
    expect(screen.queryByRole('button', { name: 'Confirm' })).toBeNull();
    await fireEvent.click(screen.getByRole('button', { name: 'Delete' }));
    expect(screen.getByRole('button', { name: 'Confirm' })).toBeTruthy();
    await fireEvent.click(screen.getByRole('button', { name: 'Cancel delete' }));
    expect(screen.queryByRole('button', { name: 'Confirm' })).toBeNull();
    expect(transport.deleteOrder).not.toHaveBeenCalled();
    // The listing is still there, and still fixable.
    expect(screen.getByRole('button', { name: 'Delete' })).toBeTruthy();
  });

  it('keeps focus on the control that replaces the one that was activated', async () => {
    const transport = makeTransport();
    const ownedQty = new Map([['primed_flow|', 1], ['ash_prime_blueprint|', 0]]);
    render(MyOrdersPanel, { props: { transport, ownedQty } });
    await screen.findByText('1 not owned');

    const remove = screen.getByRole('button', { name: 'Delete' });
    remove.focus();
    await fireEvent.click(remove);
    // Arming replaces the button the user was on; focus must follow it rather
    // than dropping to the document body.
    expect(document.activeElement).toBe(screen.getByRole('button', { name: 'Confirm' }));

    await fireEvent.click(screen.getByRole('button', { name: 'Cancel delete' }));
    expect(document.activeElement).toBe(screen.getByRole('button', { name: 'Delete' }));
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

  it('states a snapshot drift as a change to your ask, with the snapshot\'s real age', async () => {
    const book = { vol: 20, low_sell: 50, avg: 50, median_now: 50, median_90d: 50 };
    const market = { updated_at: '2026-10-02T12:00:00Z', items: { primed_flow: book, ash_prime_blueprint: { ...book, low_sell: 30, avg: 30, median_now: 30, median_90d: 30 } } } as unknown as Market;
    const transport = makeTransport({ fetchOrders: vi.fn().mockResolvedValue([
      { ...ORDERS[0], platinum: 90 },
      { ...ORDERS[1], platinum: 50, quantity: 6, per_trade: 6 },
    ]) });
    render(MyOrdersPanel, { props: { transport, market, marketStaleness: '3 d ago' } });
    const cut = await screen.findByText(/44% cut, snapshot/);
    expect(cut.getAttribute('title')).toBe("Matching the last snapshot's clearing price (50p) cuts your ask by 44% - a starting point, not a quote.");
    // A 50p lot of 6 is 8.33p a unit, not 8.333333333333334p.
    expect(screen.getByText('8.33p →', { exact: false })).toBeTruthy();
    expect(screen.getByText(/last market snapshot \(updated 3 d ago\)/)).toBeTruthy();
    expect(screen.queryByText(/up to 2 h old/)).toBeNull();
  });

  it('reprices a bulk order with a lot total while comparing fractional unit prices', async () => {
    installTauri(vi.fn().mockResolvedValue([{
      slug: 'arcane_energize', rank: 0, subtype: null, sells: [7.2], buys: [],
      low_sell: 7.2, top_buy: null, own_ask: 8, own_bid: null, error: null,
    }]), undefined);
    const transport = makeTransport({ fetchOrders: vi.fn().mockResolvedValue([{
      id: 'bulk', platinum: 48, quantity: 12, visible: false, rank: 0,
      name: 'Arcane Energize', slug: 'arcane_energize', item_id: 'arcane_energize', side: 'sell' as const, per_trade: 6, subtype: null,
    }]) });
    render(MyOrdersPanel, { props: { transport } });
    await screen.findByText('Arcane Energize');
    await fireEvent.click(screen.getByRole('button', { name: 'Check live' }));
    await screen.findByText('1 above the market');
    await fireEvent.click(screen.getByRole('button', { name: 'Reprice' }));
    await waitFor(() => expect(transport.updateOrder).toHaveBeenCalledWith('bulk', { platinum: 44 }));
  });

  // A request the transport managed to send is not a change WFM accepted. The
  // panel used to apply the local edit anyway, so the row showed a quantity the
  // live listing did not have.
  it('leaves the row alone when the remote outcome was not confirmed', async () => {
    const transport = makeTransport({
      updateOrder: vi.fn().mockResolvedValue({ status: 'pending', message: 'not sent yet' }),
    });
    const ownedQty = new Map([['primed_flow|', 1], ['ash_prime_blueprint|', 2]]);
    render(MyOrdersPanel, { props: { transport, ownedQty } });
    await screen.findByText('1 over-quantity');
    await fireEvent.click(screen.getByRole('button', { name: 'Set qty' }));
    await waitFor(() => expect(transport.updateOrder).toHaveBeenCalled());
    // Still listed ×3 against 2 owned, because nothing confirmed the change.
    expect(screen.queryByText('1 over-quantity')).not.toBeNull();
  });

  it('applies bulk visibility only to the rows WFM confirmed', async () => {
    const transport = makeTransport({
      bulkVisibility: vi.fn().mockResolvedValue({
        results: [
          { order_id: 'o1', status: 'ok' },
          { order_id: 'o2', status: 'error', message: 'WFM said no' },
        ],
      }),
    });
    render(MyOrdersPanel, { props: { transport } });
    await screen.findByText('Primed Flow');
    expect(screen.queryAllByTitle('Click to make visible')).toHaveLength(0);
    await fireEvent.click(screen.getByRole('button', { name: 'All hidden' }));
    await waitFor(() => expect(transport.bulkVisibility).toHaveBeenCalled());
    // Only the confirmed row is hidden; o2 still says ON.
    expect(screen.getAllByTitle('Click to make visible')).toHaveLength(1);
  });

  // A prime set is assembled from parts, so a scan of individual items can
  // never contain the set itself. Its absence from the owned map is therefore
  // absence of evidence, not evidence of absence - and the verdict it used to
  // produce offered a one-click delete of a live listing the user can build.
  const SET_ORDER = [
    { id: 'set1', platinum: 120, visible: true, quantity: 1, rank: 0, name: 'Ash Prime Set', slug: 'ash_prime_set', item_id: 'ash_prime_set', side: 'sell' as const, per_trade: null, subtype: null },
  ];
  const SET_MARKET = { set_to_parts: { ash_prime_set: { parts: [{ slug: 'ash_prime_blueprint', quantity: 1 }] } } };

  it('stays silent about a listed set the scan cannot report, and offers no delete', async () => {
    const transport = makeTransport({ fetchOrders: vi.fn().mockResolvedValue(SET_ORDER) });
    // The scan holds a component of the set, never the assembled set.
    const ownedQty = new Map([['ash_prime_blueprint|', 1]]);
    render(MyOrdersPanel, { props: { transport, ownedQty, market: SET_MARKET as unknown as Market } });
    // The set appears in the orders table either way; wait for the load to settle.
    await screen.findAllByText('Ash Prime Set');
    expect(screen.queryByText(/not owned/)).toBeNull();
    expect(screen.queryByRole('button', { name: 'Delete' })).toBeNull();
  });

  it('still flags a set the scan holds real evidence about', async () => {
    const transport = makeTransport({ fetchOrders: vi.fn().mockResolvedValue(SET_ORDER) });
    // Present in the map with a real count: the evidence exists, so assess it.
    const ownedQty = new Map([['ash_prime_set|', 0]]);
    render(MyOrdersPanel, { props: { transport, ownedQty, market: SET_MARKET as unknown as Market } });
    await screen.findByText('1 not owned');
  });
});

describe('MyOrdersPanel after a failed refresh', () => {
  it('keeps the last confirmed rows readable and pauses changes', async () => {
    const fetchOrders = vi.fn().mockResolvedValueOnce(ORDERS).mockRejectedValueOnce(new Error('network down'));
    render(MyOrdersPanel, { props: { transport: makeTransport({ fetchOrders }) } });
    await waitFor(() => expect(screen.getByText('Primed Flow')).toBeTruthy());
    await fireEvent.click(screen.getByRole('button', { name: 'Refresh' }));
    await waitFor(() => expect(screen.getByRole('alert').textContent).toContain("Couldn't refresh orders"));
    expect(screen.getByText('Primed Flow')).toBeTruthy();
    expect((screen.getByRole('button', { name: 'Delete Primed Flow' }) as HTMLButtonElement).disabled).toBe(true);
  });
});
