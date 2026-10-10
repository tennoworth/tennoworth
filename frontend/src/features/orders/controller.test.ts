import { afterEach, expect, it, vi } from 'vitest';
import { flushSync } from 'svelte';
import { controllerTestRoot, reactiveBox } from '../../dev/controller-test-root.svelte';
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
