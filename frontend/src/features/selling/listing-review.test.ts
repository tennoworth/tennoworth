import { afterEach, expect, it, vi } from 'vitest';
import { flushSync } from 'svelte';
import { controllerTestRoot } from '../../dev/controller-test-root.svelte';
import { createListingReview } from './listing-review.svelte';
import { ListingController } from './controller.svelte';
import type { DesktopCapabilities } from '../../contracts/desktop';
import type { DesktopServices } from '../../contracts/services';
import type { ListingCandidate } from '../../contracts/listing';
import type { OwnOrder } from '../../contracts/generated/desktop';
import type { TradeSessionState, PlanResponse } from '../../contracts/data';

function deferred<T>() {
  let resolve!: (value: T) => void;
  return { promise: new Promise<T>(yes => { resolve = yes; }), resolve };
}
const settle = () => new Promise(resolve => setTimeout(resolve, 0));
const rows: ListingCandidate[] = [{ slug: 'primed_flow', name: 'Primed Flow', owned: 3, sellable: 3, low_sell: 20, avg_price: 20, inventory_snapshot_id: 1 }];
const sessionRows: ListingCandidate[] = rows.map(row => ({ ...row, session: { snapshot_id: 1, utc_day: 20728, budget: 3 } }));
const state: TradeSessionState = { allowance: { remaining: 3, snapshot_id: 1, utc_day: 20728 }, quantities: { primed_flow: 3 }, bulk_slugs: [] } as unknown as TradeSessionState;
const cleanups: (() => void)[] = [];
afterEach(() => { for (const stop of cleanups.splice(0)) stop(); });

function setup(candidates: ListingCandidate[], port: Partial<DesktopCapabilities> = {}, services: Partial<DesktopServices> = {}) {
  const listing = new ListingController({ getPendingPlan: vi.fn(), resumePendingPlan: vi.fn(), discardPendingPlan: vi.fn(), status: vi.fn(), logout: vi.fn() }, () => {});
  listing.listingOpen = true;
  listing.reviewRowsOverride = candidates;
  const transport = { fetchOrders: vi.fn().mockResolvedValue([]), submitPlan: vi.fn().mockResolvedValue({ results: [] }), ...port } as DesktopCapabilities;
  let controller!: ReturnType<typeof createListingReview>;
  const stop = controllerTestRoot(() => {
    controller = createListingReview({
      transport, get open() { return listing.listingOpen; }, set open(value) { listing.listingOpen = value; },
      get rows() { return listing.reviewRowsOverride ?? []; }, sendThrough: send => send(),
    }, { desktopAccessStatus: vi.fn(), desktopLiveTopPrices: vi.fn(), listenForTauriEvent: vi.fn(() => () => {}), desktopTradeSessionState: vi.fn().mockResolvedValue(state), ...services });
  });
  cleanups.push(() => { controller.dispose(); stop(); });
  flushSync();
  return { c: controller, listing, transport };
}

it('ignores session evidence from a review that was closed and reopened', async () => {
  const old = deferred<TradeSessionState>();
  const desktopTradeSessionState = vi.fn().mockReturnValueOnce(old.promise).mockResolvedValue(state);
  const { c, listing } = setup(sessionRows, {}, { desktopTradeSessionState });
  listing.listingOpen = false;
  flushSync();
  listing.listingOpen = true;
  flushSync();
  await settle();
  expect(c.ordersReady).toBe(true);
  old.resolve({ ...state, quantities: { primed_flow: 0 }, allowance: { ...state.allowance, remaining: 0 } });
  await settle();
  expect(c.sessionRemaining).toBe(3);
  expect(c.plan[0].sellable).toBe(3);
});

it('ignores an old orders response and keeps the reopened review busy until its own response', async () => {
  const old = deferred<OwnOrder[]>();
  const latest = deferred<OwnOrder[]>();
  const fetchOrders = vi.fn().mockReturnValueOnce(old.promise).mockReturnValueOnce(latest.promise);
  const { c, listing } = setup(sessionRows, { fetchOrders });
  await settle();
  expect(fetchOrders).toHaveBeenCalledTimes(1);
  listing.listingOpen = false;
  flushSync();
  listing.listingOpen = true;
  flushSync();
  await settle();
  expect(fetchOrders).toHaveBeenCalledTimes(2);
  expect(c.ordersBusy).toBe(true);
  old.resolve([]);
  await settle();
  expect(c.ordersReady).toBe(false);
  expect(c.ordersBusy).toBe(true);
  latest.resolve([]);
  await settle();
  expect(c.ordersReady).toBe(true);
  expect(c.ordersBusy).toBe(false);
});

it('keeps draft edits when the caller recomputes rows during an open review', () => {
  const { c, listing } = setup(rows);
  c.plan[0].platinum = 37;
  c.plan[0].quantity = 2;
  listing.reviewRowsOverride = rows.map(row => ({ ...row, low_sell: 11 }));
  flushSync();
  expect(c.plan[0].platinum).toBe(37);
  expect(c.plan[0].quantity).toBe(2);
});

it('holds sending busy and prevents a second submission while IPC is pending', async () => {
  const pending = deferred<PlanResponse>();
  const submitPlan = vi.fn(() => pending.promise);
  const { c } = setup(rows, { submitPlan });
  const first = c.send();
  await settle();
  expect(c.phase).toBe('sending');
  const duplicate = c.send();
  await settle();
  expect(submitPlan).toHaveBeenCalledTimes(1);
  pending.resolve({ plan_id: 'batch', results: [] });
  await Promise.all([first, duplicate]);
  expect(c.phase).toBe('results');
  expect(c.validatingSend).toBe(false);
});
