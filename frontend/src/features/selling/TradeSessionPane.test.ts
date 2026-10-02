import { afterEach, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import TradeSessionPane from './TradeSessionPane.svelte';
import type { SessionPlan } from '../../domain/trade-session';
import type { Market, OwnedRecord } from '../../contracts/data';
import type { ScoredInventoryFact } from '../../contracts/generated/domain';

const services = vi.hoisted(() => ({
  evaluateTradeSession: vi.fn(),
  desktopTradeSessionState: vi.fn(),
  desktopLiveTopPrices: vi.fn(),
  listenForTauriEvent: vi.fn(() => () => {}),
}));
vi.mock('../../ui/desktop-context', () => ({ useDesktopServices: () => services }));
afterEach(() => { cleanup(); vi.clearAllMocks(); });

type Plan = SessionPlan;
function plan(name: string): Plan {
  return { rows: [{ key: name, slug: name, name, owned: 2, sellable: 2, leveled: 0, type: 'Mod', hold: false, bulk: false,
    market: { avg: 10, low_sell: 10, vol: 30, top_buy: 0, buys: 0, sells: 0, ratio: 0 },
    component_limits: { [name]: 2 }, quantity: 1, per_trade: 1, platinum: 10, trades: 1, reason: 'Liquid singles; match credible asks.', bid: null }],
    trades: 1, total: 10, excluded: [], target: null, shortfall: null };
}
async function mountPane() {
  services.desktopTradeSessionState.mockResolvedValue({ allowance: { remaining: 8, snapshot_id: 'one' }, quantities: {}, bulk_slugs: [] });
  services.evaluateTradeSession.mockImplementation(async () => plan('Initial'));
  const onreview = vi.fn();
  const view = render(TradeSessionPane, { props: { owned: new Map(), market: null, reserveCopies: 0, advice: new Map(), scanning: false, onscan: async () => {}, onreview } });
  await waitFor(() => expect((screen.getByRole('button', { name: 'Review batch' }) as HTMLButtonElement).disabled).toBe(false));
  return { ...view, onreview };
}
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

describe('native Trade Session planning', () => {
  it('disables review while pending and ignores a stale successful reply', async () => {
    const { onreview } = await mountPane();
    const older = deferred<Plan>();
    const newer = deferred<Plan>();
    services.evaluateTradeSession.mockImplementation(({ mode }) => mode === 'clear' ? older.promise : newer.promise);
    await fireEvent.click(screen.getByRole('button', { name: /^Clear Inventory/ }));
    expect((screen.getByRole('button', { name: 'Review batch' }) as HTMLButtonElement).disabled).toBe(true);
    expect(screen.getByText('Updating suggested batch…')).toBeTruthy();
    await fireEvent.click(screen.getByRole('button', { name: /^Max Value/ }));
    newer.resolve(plan('Current'));
    await waitFor(() => expect(screen.getByText('Current')).toBeTruthy());
    older.resolve(plan('Stale'));
    await Promise.resolve();
    expect(screen.queryByText('Stale')).toBeNull();
    await fireEvent.click(screen.getByRole('button', { name: 'Review batch' }));
    expect(onreview).toHaveBeenCalledWith(expect.arrayContaining([expect.objectContaining({ name: 'Current' })]), 8, expect.anything());
  });

  it('retains prior rows but blocks review after rejection and clears the error on a successful retry', async () => {
    await mountPane();
    services.evaluateTradeSession.mockRejectedValue(new Error('Native planner unavailable'));
    await fireEvent.click(screen.getByRole('button', { name: /^Clear Inventory/ }));
    await waitFor(() => expect(screen.getByRole('alert').textContent).toContain('Native planner unavailable'));
    expect(screen.getByText('Initial')).toBeTruthy();
    expect((screen.getByRole('button', { name: 'Review batch' }) as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByRole('button', { name: 'Check live prices' }) as HTMLButtonElement).disabled).toBe(true);
    services.evaluateTradeSession.mockResolvedValue(plan('Recovered'));
    await fireEvent.click(screen.getByRole('button', { name: /^Max Value/ }));
    await waitFor(() => expect(screen.getByText('Recovered')).toBeTruthy());
    expect(screen.queryByRole('alert')).toBeNull();
    expect((screen.getByRole('button', { name: 'Review batch' }) as HTMLButtonElement).disabled).toBe(false);
  });

  it('ignores a stale rejection and replies delivered after unmount', async () => {
    const { unmount } = await mountPane();
    const old = deferred<Plan>();
    services.evaluateTradeSession.mockImplementation(({ mode }) => mode === 'clear' ? old.promise : Promise.resolve(plan('Current')));
    await fireEvent.click(screen.getByRole('button', { name: /^Clear Inventory/ }));
    await fireEvent.click(screen.getByRole('button', { name: /^Max Value/ }));
    await waitFor(() => expect(screen.getByText('Current')).toBeTruthy());
    old.reject(new Error('Old failure'));
    await Promise.resolve();
    expect(screen.queryByRole('alert')).toBeNull();
    const late = deferred<Plan>();
    services.evaluateTradeSession.mockReturnValue(late.promise);
    await fireEvent.click(screen.getByRole('button', { name: /^Clear Inventory/ }));
    await unmount();
    late.resolve(plan('Late'));
    await Promise.resolve();
    expect(screen.queryByText('Late')).toBeNull();
  });
});

it('explains blocked review and restores the action without discarding the draft', async () => {
  const view = await mountPane();
  await view.rerender({ listingBlockReason: 'Connect WFM to check current listings before posting.' });
  expect((screen.getByRole('button', { name: 'Review batch' }) as HTMLButtonElement).disabled).toBe(true);
  expect(screen.getByText('Connect WFM to check current listings before posting.')).toBeTruthy();
  await fireEvent.click(screen.getByRole('button', { name: 'Review batch' }));
  expect(view.onreview).not.toHaveBeenCalled();
  await view.rerender({ listingBlockReason: null });
  await fireEvent.click(screen.getByRole('button', { name: 'Review batch' }));
  expect(view.onreview).toHaveBeenCalledOnce();
});

it.each([
  { error: null, low_sell: 0, top_buy: 0, note: 'Online competitors checked' },
  { error: 'Unavailable', low_sell: 10, top_buy: 8, note: '1 price checks failed' },
])('preserves Trade Session live-book semantics ($error)', async expected => {
  services.desktopTradeSessionState.mockResolvedValue({ allowance: { remaining: 8, snapshot_id: 'one' }, quantities: { Initial: 2 }, bulk_slugs: [] });
  services.evaluateTradeSession.mockImplementation(async () => plan('Initial'));
  services.desktopLiveTopPrices.mockResolvedValue([{ slug: 'Initial', low_sell: null, top_buy: null, sells: [], buys: [], error: expected.error }]);
  const market: Market = { updated_at: '2026-10-02T00:00:00Z', platform: 'pc', item_count: 1, catalog_count: 1, catalog: { initial: 'Initial' }, items: { Initial: { avg: 10, low_sell: 10, top_buy: 8, vol: 30, buys: 1, sells: 1, ratio: 1 } } };
  const owned = new Map<string, OwnedRecord>([['Initial', { slug: 'Initial', name: 'Initial', type: 'Mod', count: 2, kept_lvl: null, leveled: 0, subtype: null }]]);
  const nativeFacts = new Map<string, ScoredInventoryFact>([['Initial', {
    key: 'Initial', sellable: 2, clearing_price: 10, sell_score: 20, patience: false,
    ducats: null, plat_per_100d: null, potential_plat: 20, raw_value: 20,
    medians_7d: [], median_90d: null, delta_90d_pct: null, timing: 'neutral',
    demand: { usage: null, inherited: false, liquidity: 'sells-today', band: null },
  }]]);
  render(TradeSessionPane, { props: { owned, market, nativeFacts, reserveCopies: 0, advice: new Map(), scanning: false, onscan: async () => {}, onreview: vi.fn() } });
  const check = screen.getByRole('button', { name: 'Check live prices' });
  await waitFor(() => expect((check as HTMLButtonElement).disabled).toBe(false));
  const previousCalls = services.evaluateTradeSession.mock.calls.length;
  await fireEvent.click(check);
  await screen.findByText(new RegExp(expected.note));
  if (!expected.error) await waitFor(() => expect(services.evaluateTradeSession.mock.calls.length).toBeGreaterThan(previousCalls));
  await waitFor(() => expect((screen.getByRole('button', { name: 'Review batch' }) as HTMLButtonElement).disabled).toBe(false));
  expect(services.evaluateTradeSession.mock.calls.at(-1)?.[0].candidates[0].market).toMatchObject({ low_sell: expected.low_sell, top_buy: expected.top_buy });
  expect(services.desktopLiveTopPrices).toHaveBeenCalledWith([{ slug: 'Initial', rank: 0, subtype: null }]);
});
