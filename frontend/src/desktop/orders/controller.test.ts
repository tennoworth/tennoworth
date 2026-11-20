import { afterEach, expect, it, vi } from 'vitest';
import { flushSync } from 'svelte';
import { controllerTestRoot, reactiveBox } from '../../dev/testing/controller-test-root.svelte';
import { createOrdersController } from './controller.svelte';
import { DesktopCmdError } from '../../contracts/errors';
import type { DesktopCapabilities } from '../../contracts/desktop';
import type { OwnOrder } from '../../contracts/generated/desktop';

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  return { promise: new Promise<T>((yes, no) => { resolve = yes; reject = no; }), resolve, reject };
}
const settle = () => new Promise(resolve => setTimeout(resolve, 0));
const order: OwnOrder = { id: 'one', item_id: 'one', slug: 'primed_flow', name: 'Primed Flow', platinum: 20, quantity: 1, visible: true, rank: 0, side: 'sell', per_trade: null, subtype: null };
const cleanups: (() => void)[] = [];
afterEach(() => { for (const stop of cleanups.splice(0)) stop(); });

function setup(port: Partial<DesktopCapabilities>) {
  let controller!: ReturnType<typeof createOrdersController>;
  const stop = controllerTestRoot(() => {
    controller = createOrdersController({ transport: { fetchOrders: vi.fn().mockResolvedValue([order]), ...port } as DesktopCapabilities }, {
      desktopLiveTopPrices: vi.fn(), listenForTauriEvent: vi.fn(() => () => {}),
    });
  });
  cleanups.push(() => { controller.dispose(); stop(); });
  flushSync();
  return controller;
}

it.each(['success', 'failure'] as const)('ignores a superseded orders %s while keeping the newest response', async outcome => {
  const old = deferred<OwnOrder[]>();
  const latest = deferred<OwnOrder[]>();
  const fetchOrders = vi.fn().mockReturnValueOnce(old.promise).mockReturnValueOnce(latest.promise);
  const c = setup({ fetchOrders });
  expect(c.phase).toBe('loading');
  c.loadOrders();
  latest.resolve([{ ...order, id: 'new' }]);
  await settle();
  if (outcome === 'success') old.resolve([{ ...order, id: 'old' }]);
  else old.reject(new Error('old request failed'));
  await settle();
  expect(c.orders.map(row => row.id)).toEqual(['new']);
  expect(c.phase).toBe('done');
  expect(c.error).toBeNull();
});

it('holds a row busy through an IPC rejection and retains the confirmed value', async () => {
  const update = deferred<unknown>();
  const c = setup({ updateOrder: vi.fn(() => update.promise) });
  await settle();
  const toggle = c.toggleVisible(c.orders[0]);
  expect(c.busyIds.has('one')).toBe(true);
  update.reject(new Error('offline'));
  await toggle;
  expect(c.busyIds.has('one')).toBe(false);
  expect(c.orders[0].visible).toBe(true);
  expect(c.toasts).toEqual([expect.objectContaining({ kind: 'error', text: "Couldn't toggle: offline" })]);
});

it('holds bulk visibility busy and refuses an overlapping action', async () => {
  const update = deferred<{ results: [] }>();
  const bulkVisibility = vi.fn(() => update.promise);
  const c = setup({ bulkVisibility });
  await settle();
  const first = c.bulkSetVisible(false);
  expect(c.bulkBusy).toBe(true);
  const duplicate = c.bulkSetVisible(false);
  expect(bulkVisibility).toHaveBeenCalledTimes(1);
  update.resolve({ results: [] });
  await Promise.all([first, duplicate]);
  expect(c.bulkBusy).toBe(false);
  expect(c.orders[0].visible).toBe(true);
});

it('refreshes when the orders view comes back on screen, and only then', async () => {
  const fetchOrders = vi.fn().mockResolvedValue([order]);
  const active = reactiveBox(true);
  let controller!: ReturnType<typeof createOrdersController>;
  const stop = controllerTestRoot(() => {
    controller = createOrdersController({ transport: { fetchOrders } as unknown as DesktopCapabilities, get active() { return active.value; } }, {
      desktopLiveTopPrices: vi.fn(), listenForTauriEvent: vi.fn(() => () => {}),
    });
  });
  cleanups.push(() => { controller.dispose(); stop(); });
  flushSync();
  await settle();
  expect(fetchOrders).toHaveBeenCalledTimes(1);
  active.value = false;
  flushSync();
  expect(fetchOrders).toHaveBeenCalledTimes(1);
  active.value = true;
  flushSync();
  expect(fetchOrders).toHaveBeenCalledTimes(2);
  // The rows already on screen stay while the refresh runs.
  expect(controller.orders.map((row) => row.id)).toEqual(['one']);
});

it('does not ask for sign-in from the hidden orders view after a logout elsewhere', async () => {
  const active = reactiveBox(true);
  const epoch = reactiveBox(0);
  const auth = vi.fn();
  const fetchOrders = vi.fn().mockResolvedValueOnce([order]).mockRejectedValue(new DesktopCmdError('needs_login', 'Login required'));
  let controller!: ReturnType<typeof createOrdersController>;
  const stop = controllerTestRoot(() => {
    controller = createOrdersController({ transport: { fetchOrders } as unknown as DesktopCapabilities, onauthrequired: auth, get active() { return active.value; }, get sessionEpoch() { return epoch.value; } }, {
      desktopLiveTopPrices: vi.fn(), listenForTauriEvent: vi.fn(() => () => {}),
    });
  });
  cleanups.push(() => { controller.dispose(); stop(); });
  flushSync();
  await settle();
  active.value = false;
  flushSync();
  // Logging out from Settings bumps the session epoch.
  epoch.value += 1;
  flushSync();
  await settle();
  expect(auth).not.toHaveBeenCalled();
  expect(fetchOrders).toHaveBeenCalledTimes(1);
  // The old session's rows are gone, and returning asks then.
  expect(controller.orders).toEqual([]);
  active.value = true;
  flushSync();
  await settle();
  expect(auth).toHaveBeenCalledWith('needs_login');
});

it.each([false, true])('never shows the previous account a live quote after a session change (in flight: %s)', async inFlight => {
  const epoch = reactiveBox(0);
  const active = reactiveBox(true);
  let account = 'account-a';
  const oldQuote = deferred<unknown[]>();
  const quote = { slug: 'primed_flow', rank: 0, low_sell: 99, top_buy: 15, sells: [99], buys: [15], own_ask: 20 };
  const desktopLiveTopPrices = vi.fn(() => inFlight ? oldQuote.promise : Promise.resolve([quote]));
  let controller!: ReturnType<typeof createOrdersController>;
  const stop = controllerTestRoot(() => {
    controller = createOrdersController({
      transport: { fetchOrders: async () => [{ ...order, id: account }] } as unknown as DesktopCapabilities,
      get active() { return active.value; },
      get sessionEpoch() { return epoch.value; },
    }, { desktopLiveTopPrices, listenForTauriEvent: vi.fn(() => () => {}) } as never);
  });
  cleanups.push(() => { controller.dispose(); stop(); });
  await settle();
  const check = controller.checkLive();
  if (!inFlight) await check;
  active.value = false; flushSync(); await settle();
  epoch.value += 1; flushSync(); await settle();
  account = 'account-b';
  epoch.value += 1; active.value = true; flushSync(); await settle();
  if (inFlight) { oldQuote.resolve([quote]); await check; await settle(); }
  expect(controller.orders.map(row => row.id)).toEqual(['account-b']);
  expect(controller.liveForOrder(controller.orders[0])).toBeNull();
  expect(controller.liveTop.quotes.size).toBe(0);
  expect(desktopLiveTopPrices).toHaveBeenCalledTimes(1);
});

it.each([
  [null, 4, false],
  [null, 5, true],
  [3, 14, false],
  [3, 15, true],
])('refuses a price edit below the per-unit floor (lot %s, %sp)', async (perTrade, price, sent) => {
  const updateOrder = vi.fn().mockResolvedValue({ status: 'ok' });
  const c = setup({ fetchOrders: vi.fn().mockResolvedValue([{ ...order, per_trade: perTrade }]), updateOrder });
  await settle();
  c.startEdit(c.orders[0]);
  c.editValue = price;
  await c.saveEdit(c.orders[0]);
  expect(updateOrder).toHaveBeenCalledTimes(sent ? 1 : 0);
  if (!sent) expect(c.toasts).toEqual([expect.objectContaining({ kind: 'error', text: expect.stringContaining('minimum for this lot') })]);
});

function priced(orders: OwnOrder[], quotes: unknown[], market: unknown, rule = { basis: 'low5-avg' as const, offset: 0 }) {
  const updateOrder = vi.fn().mockResolvedValue({ status: 'ok' });
  let controller!: ReturnType<typeof createOrdersController>;
  const stop = controllerTestRoot(() => {
    controller = createOrdersController({
      transport: { fetchOrders: vi.fn().mockResolvedValue(orders), updateOrder } as unknown as DesktopCapabilities,
      market: market as never,
      healthPrefs: { rule, tolerance: 0, muted: [] },
    }, { desktopLiveTopPrices: vi.fn().mockResolvedValue(quotes), listenForTauriEvent: vi.fn(() => () => {}) } as never);
  });
  cleanups.push(() => { controller.dispose(); stop(); });
  flushSync();
  return { controller, updateOrder };
}

it('reprices to a whole-platinum target and the flag clears', async () => {
  const market = { items: { primed_flow: { price_basis: 'unit', low5_avg: 15.4, avg: 16, low_sell: 15, top_buy: 0, vol: 40, ratio: 1, buys: 0, sells: 0 } } };
  const quote = { slug: 'primed_flow', rank: 0, low_sell: 15, top_buy: null, sells: [15, 16], buys: [] };
  const { controller: c, updateOrder } = priced([{ ...order, platinum: 18 }], [quote], market);
  await settle();
  await c.checkLive();
  flushSync();
  expect(c.health).toEqual([expect.objectContaining({ kind: 'overpriced', suggestedLot: 16 })]);
  await c.applyFix(c.health[0]);
  flushSync();
  expect(updateOrder).toHaveBeenCalledWith('one', { platinum: 16 });
  expect(c.health).toEqual([]);
});

it('judges a ranked listing by its own tier, before and after a live check', async () => {
  const market = { items: { arcane_momentum: { price_basis: 'unit', low5_avg: 3.2, avg: 3, low_sell: 1, top_buy: 0, vol: 6, ratio: 1, buys: 0, sells: 0, median_90d: 2, median_now: 2 } } };
  const momentum: OwnOrder = { ...order, id: 'momentum', slug: 'arcane_momentum', name: 'Arcane Momentum', platinum: 21, rank: 5 };
  const quote = { slug: 'arcane_momentum', rank: 5, low_sell: 20, top_buy: null, sells: [20, 22, 24], buys: [] };
  const { controller: c } = priced([momentum], [quote], market);
  await settle();
  expect(c.drifted).toEqual([]);
  await c.checkLive();
  flushSync();
  expect(c.health).toEqual([]);
});

it('prices a maxed listing from the snapshot max-rank tier, and no other rank', async () => {
  const ranked = { rank: 5, avg: 25, low_sell: 24, low5_avg: 26, low20_avg: 28, top_buy: 15, vol: 12, buys: 3, sells: 9, median_now: 25, median_90d: 25 };
  const market = { items: { arcane_momentum: { price_basis: 'unit', low5_avg: 3.2, avg: 3, low_sell: 1, top_buy: 0, vol: 6, ratio: 1, buys: 0, sells: 0, median_90d: 2, median_now: 2, ranked } } };
  const maxed: OwnOrder = { ...order, id: 'maxed', slug: 'arcane_momentum', name: 'Arcane Momentum', platinum: 18, rank: 5 };
  const partial: OwnOrder = { ...maxed, id: 'partial', platinum: 11, rank: 3 };
  const quotes = [
    { slug: 'arcane_momentum', rank: 5, low_sell: 20, top_buy: null, sells: [20, 22, 24], buys: [] },
    { slug: 'arcane_momentum', rank: 3, low_sell: 10, top_buy: null, sells: [10, 11, 12], buys: [] },
  ];
  const { controller: c } = priced([maxed, partial], quotes, market);
  await settle();
  expect(c.drifted.map((row) => row.id)).toEqual(['maxed']);
  await c.checkLive();
  flushSync();
  expect(c.health).toEqual([expect.objectContaining({ id: 'maxed', kind: 'underpriced', suggestedLot: 26 })]);
});

it('leaves raising an underpriced listing out of Fix all prices', async () => {
  const market = { items: { primed_flow: { price_basis: 'unit', low5_avg: 25, avg: 25, low_sell: 25, top_buy: 0, vol: 40, ratio: 1, buys: 0, sells: 0 } } };
  const quote = { slug: 'primed_flow', rank: 0, low_sell: 25, top_buy: null, sells: [25], buys: [] };
  const { controller: c, updateOrder } = priced([{ ...order, platinum: 10 }], [quote], market);
  await settle();
  await c.checkLive();
  flushSync();
  expect(c.health).toEqual([expect.objectContaining({ kind: 'underpriced', suggestedLot: 25 })]);
  await c.fixAllPrices();
  expect(updateOrder).not.toHaveBeenCalled();
  await c.applyFix(c.health[0]);
  expect(updateOrder).toHaveBeenCalledWith('one', { platinum: 25 });
});
