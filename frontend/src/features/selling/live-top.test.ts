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

function setup() {
  let progress: (value: { done: number; total: number }) => void = () => {};
  const stop = vi.fn();
  const services = {
    desktopLiveTopPrices: vi.fn().mockResolvedValue([]),
    listenForTauriEvent: vi.fn((_event, callback) => { progress = callback; return stop; }),
  };
  return { state: new LiveTopController(services), services, stop, emit: (done: number, total: number) => progress({ done, total }) };
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
});
