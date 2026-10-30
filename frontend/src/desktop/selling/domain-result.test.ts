import { describe, expect, it } from 'vitest';
import { DomainResult } from './domain-result.svelte';

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  return { promise: new Promise<T>((yes, no) => { resolve = yes; reject = no; }), resolve, reject };
}
const settle = () => new Promise(resolve => setTimeout(resolve, 0));

describe('native calculation lifecycle', () => {
  it('keeps only the newest response when inputs change', async () => {
    const first = deferred<string[]>();
    const second = deferred<string[]>();
    const state = new DomainResult<string[]>(() => []);
    const stop = state.start(() => first.promise);
    stop();
    state.start(() => second.promise);
    second.resolve(['new']);
    await settle();
    first.resolve(['old']);
    await settle();
    expect(state.value).toEqual(['new']);
    expect(state.phase).toBe('done');
  });

  it('cannot revive cleared inventory after a request completes', async () => {
    const pending = deferred<string[]>();
    const state = new DomainResult<string[]>(() => []);
    state.start(() => pending.promise);
    state.clear();
    pending.resolve(['old account']);
    await settle();
    expect(state.value).toEqual([]);
    expect(state.phase).toBe('idle');
  });

  it('distinguishes a calculation failure from a successful empty result', async () => {
    const state = new DomainResult<string[]>(() => []);
    state.start(async () => { throw new Error('Native calculation unavailable'); });
    expect(state.phase).toBe('loading');
    await settle();
    expect(state.phase).toBe('error');
    expect(state.error).toBe('Native calculation unavailable');
    expect(state.value).toEqual([]);
    state.start(async () => []);
    await settle();
    expect(state.phase).toBe('done');
    expect(state.error).toBeNull();
  });

  it('ignores a stale rejection after a newer successful calculation', async () => {
    const old = deferred<string[]>();
    const state = new DomainResult<string[]>(() => []);
    state.start(() => old.promise);
    state.start(async () => ['ready']);
    await settle();
    old.reject(new Error('stale'));
    await settle();
    expect(state.value).toEqual(['ready']);
    expect(state.phase).toBe('done');
    expect(state.error).toBeNull();
  });
});
