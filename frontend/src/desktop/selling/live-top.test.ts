import { describe, expect, it, vi } from 'vitest';
import type { LiveTop } from '../../contracts/desktop';
import { DesktopCmdError } from '../../contracts/errors';
import { LIVE_TOP_PROGRESS_EVENT } from '../../contracts/events';
import { LiveTopController } from './live-top.svelte';

function quote(overrides: Partial<LiveTop> = {}): LiveTop {
  return { slug: 'flow', low_sell: 20, top_buy: 15, sells: [20], buys: [15], ...overrides };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  return { promise: new Promise<T>((yes, no) => { resolve = yes; reject = no; }), resolve, reject };
}

type Progress = { request_id: number; done: number; total: number };

function setup() {
  const listeners = new Set<(value: Progress) => void>();
  const stop = vi.fn();
  const services = {
    desktopLiveTopPrices: vi.fn().mockResolvedValue([]),
    listenForTauriEvent: vi.fn((_event, callback) => { listeners.add(callback); return () => { listeners.delete(callback); stop(); }; }),
  };
  const lastRequestId = (): number => services.desktopLiveTopPrices.mock.calls.at(-1)?.[1];
  const broadcast = (value: Progress) => { for (const listener of [...listeners]) listener(value); };
  return {
    state: new LiveTopController(services), services, stop, lastRequestId, broadcast,
    emit: (done: number, total: number) => broadcast({ request_id: lastRequestId(), done, total }),
  };
}

describe('shared live top-of-book lifecycle', () => {
  it('does not start a request or subscribe without targets', async () => {
    const { state, services } = setup();
    await state.check([]);
    expect(state.phase).toBe('idle');
    expect(services.desktopLiveTopPrices).not.toHaveBeenCalled();
    expect(services.listenForTauriEvent).not.toHaveBeenCalled();
  });

  it('reports progress only during a request and retains null live prices', async () => {
    const { state, services, emit } = setup();
    const pending = deferred<LiveTop[]>();
    services.desktopLiveTopPrices.mockReturnValue(pending.promise);
    const request = state.check([{ slug: 'flow' }]);
    expect(state.phase).toBe('running');
    expect(state.progress).toEqual({ done: 0, total: 1 });
    expect(services.listenForTauriEvent).toHaveBeenCalledWith(LIVE_TOP_PROGRESS_EVENT, expect.any(Function));
    emit(1, 1);
    expect(state.progress).toEqual({ done: 1, total: 1 });
    const emptyBook = quote({ low_sell: null, top_buy: null, sells: [], buys: [] });
    pending.resolve([emptyBook]);
    expect(await request).toEqual([emptyBook]);
    expect(state.get({ slug: 'flow', rank: 0, subtype: null })).toEqual(emptyBook);
    expect(state.phase).toBe('done');
    emit(50, 100);
    expect(state.progress).toEqual({ done: 1, total: 1 });
  });

  it('keeps exact tiers separate and merges rechecks without discarding other tiers', async () => {
    const { state, services } = setup();
    const quotes = [quote(), quote({ rank: 10, low_sell: 100 }), quote({ subtype: 'radiant', low_sell: 30 })];
    services.desktopLiveTopPrices.mockResolvedValue(quotes);
    await state.check([{ slug: 'flow' }, { slug: 'flow', rank: 10 }, { slug: 'flow', subtype: 'radiant' }]);
    for (const q of quotes) expect(state.get(q)).toEqual(q);
    expect(state.get({ slug: 'flow', rank: 5 })).toBeUndefined();
    const refreshed = quote({ rank: 0, subtype: null, low_sell: 21 });
    services.desktopLiveTopPrices.mockResolvedValue([refreshed]);
    await state.check([{ slug: 'flow' }]);
    expect(state.get({ slug: 'flow' })).toEqual(refreshed);
    expect(state.get({ slug: 'flow', rank: 10 })).toEqual(quotes[1]);
    expect(state.get({ slug: 'flow', subtype: 'radiant' })).toEqual(quotes[2]);
    expect(services.listenForTauriEvent).toHaveBeenCalledTimes(1);
  });

  it.each([new Error('Offline'), new DesktopCmdError('request_limited', 'Offline'), 'Offline'])('retains quotes on rejection and clears the error on retry (%s)', async error => {
    const { state, services } = setup();
    services.desktopLiveTopPrices.mockResolvedValue([quote()]);
    await state.check([{ slug: 'flow' }]);
    services.desktopLiveTopPrices.mockRejectedValue(error);
    expect(await state.check([{ slug: 'flow' }])).toBeNull();
    expect(state.phase).toBe('error');
    expect(state.error).toBe('Offline');
    expect(state.get({ slug: 'flow' })).toEqual(quote());
    services.desktopLiveTopPrices.mockResolvedValue([]);
    await state.check([{ slug: 'flow' }]);
    expect(state.phase).toBe('done');
    expect(state.error).toBeNull();
  });

  it('keeps per-item errors distinct from a failed batch', async () => {
    const { state, services } = setup();
    const failed = quote({ slug: 'odd', error: 'Tier unavailable', low_sell: null, top_buy: null });
    services.desktopLiveTopPrices.mockResolvedValue([quote(), failed]);
    expect(await state.check([{ slug: 'flow' }, { slug: 'odd' }])).toEqual([quote(), failed]);
    expect(state.phase).toBe('done');
    expect(state.error).toBeNull();
    expect(state.get({ slug: 'odd' })?.error).toBe('Tier unavailable');
  });

  it('does not overlap requests on the shared progress channel', async () => {
    const { state, services } = setup();
    const pending = deferred<LiveTop[]>();
    services.desktopLiveTopPrices.mockReturnValue(pending.promise);
    const request = state.check([{ slug: 'flow' }]);
    const duplicate = state.check([{ slug: 'odd' }]);
    expect(services.desktopLiveTopPrices).toHaveBeenCalledTimes(1);
    pending.resolve([quote()]);
    expect(await duplicate).toBeNull();
    await request;
  });

  it.each(['success', 'failure'])('unsubscribes once and ignores a late %s after disposal', async outcome => {
    const { state, services, stop, emit } = setup();
    const pending = deferred<LiveTop[]>();
    services.desktopLiveTopPrices.mockReturnValue(pending.promise);
    const request = state.check([{ slug: 'flow' }]);
    state.dispose();
    state.dispose();
    emit(1, 1);
    if (outcome === 'success') pending.resolve([quote()]);
    else pending.reject(new Error('Late error'));
    expect(await request).toBeNull();
    expect(stop).toHaveBeenCalledTimes(1);
    expect(state.quotes.size).toBe(0);
    expect(state.error).toBeNull();
    expect(state.progress).toEqual({ done: 0, total: 1 });
    await state.check([{ slug: 'odd' }]);
    expect(services.desktopLiveTopPrices).toHaveBeenCalledTimes(1);
  });

  it('ignores progress that belongs to another request on the shared channel', async () => {
    const { state, services, broadcast, lastRequestId } = setup();
    services.desktopLiveTopPrices.mockReturnValue(deferred<LiveTop[]>().promise);
    void state.check([{ slug: 'flow' }]);
    const own = lastRequestId();
    expect(own).toEqual(expect.any(Number));
    broadcast({ request_id: own + 1, done: 2, total: 3 });
    expect(state.progress).toEqual({ done: 0, total: 1 });
    broadcast({ request_id: own, done: 1, total: 1 });
    expect(state.progress).toEqual({ done: 1, total: 1 });
  });

  it('gives concurrent controllers distinct requests and separate progress', async () => {
    const shared = setup();
    const other = new LiveTopController(shared.services);
    shared.services.desktopLiveTopPrices.mockReturnValue(deferred<LiveTop[]>().promise);
    void shared.state.check([{ slug: 'a' }]);
    const first = shared.lastRequestId();
    void other.check([{ slug: 'b' }, { slug: 'c' }, { slug: 'd' }]);
    const second = shared.lastRequestId();
    expect(second).not.toBe(first);
    shared.broadcast({ request_id: second, done: 2, total: 3 });
    expect(shared.state.progress).toEqual({ done: 0, total: 1 });
    expect(other.progress).toEqual({ done: 2, total: 3 });
    other.dispose();
  });

  it('records when the retained quotes were observed', async () => {
    const { state, services } = setup();
    expect(state.checkedAt).toBeNull();
    const before = Date.now();
    services.desktopLiveTopPrices.mockResolvedValue([quote()]);
    await state.check([{ slug: 'flow' }]);
    expect(state.checkedAt).toBeGreaterThanOrEqual(before);
  });

  it('forgets completed quotes when its context resets', async () => {
    const { state, services } = setup();
    services.desktopLiveTopPrices.mockResolvedValue([quote({ own_ask: 20 })]);
    await state.check([{ slug: 'flow' }]);
    services.desktopLiveTopPrices.mockRejectedValue(new Error('Offline'));
    await state.check([{ slug: 'flow' }]);
    state.reset();
    expect(state.quotes.size).toBe(0);
    expect(state.get({ slug: 'flow' })).toBeUndefined();
    expect(state.phase).toBe('idle');
    expect(state.error).toBeNull();
    expect(state.checkedAt).toBeNull();
    expect(state.progress).toEqual({ done: 0, total: 0 });
  });

  it.each(['success', 'failure'])('drops a %s that lands after its context reset', async outcome => {
    const { state, services, emit } = setup();
    const old = deferred<LiveTop[]>();
    services.desktopLiveTopPrices.mockReturnValue(old.promise);
    const request = state.check([{ slug: 'flow' }]);
    state.reset();
    expect(state.phase).toBe('idle');
    emit(1, 1);
    expect(state.progress).toEqual({ done: 0, total: 0 });
    if (outcome === 'success') old.resolve([quote({ own_ask: 20 })]);
    else old.reject(new Error('Old account failed'));
    expect(await request).toBeNull();
    expect(state.quotes.size).toBe(0);
    expect(state.phase).toBe('idle');
    expect(state.error).toBeNull();
    expect(state.checkedAt).toBeNull();
  });

  it('accepts a new check while a reset request is still unresolved', async () => {
    const { state, services } = setup();
    const old = deferred<LiveTop[]>();
    services.desktopLiveTopPrices.mockReturnValue(old.promise);
    const stale = state.check([{ slug: 'flow' }]);
    state.reset();
    const current = quote({ low_sell: 25 });
    services.desktopLiveTopPrices.mockResolvedValue([current]);
    expect(await state.check([{ slug: 'flow' }])).toEqual([current]);
    old.resolve([quote({ own_ask: 20 })]);
    expect(await stale).toBeNull();
    expect(state.get({ slug: 'flow' })).toEqual(current);
    expect(state.phase).toBe('done');
  });
});
