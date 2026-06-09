import { describe, it, expect, vi, afterEach } from 'vitest';
import { flushSync, mount, unmount } from 'svelte';
import Repro from './__effect_repro.svelte';

describe('ResultsTable sort-fallback effect read+write', () => {
  afterEach(() => {
    document.body.innerHTML = '';
    vi.restoreAllMocks();
  });

  it('does not throw effect_update_depth_exceeded on a preset switch that hides the sorted column', () => {
    const errors: unknown[] = [];
    const errSpy = vi.spyOn(console, 'error').mockImplementation((...a) => { errors.push(a); });

    const target = document.createElement('div');
    document.body.appendChild(target);

    // Start with no preset (all columns), then sort by avg_price.
    const props = $state<{ visibleColumns: string[] | null }>({ visibleColumns: null });
    const comp = mount(Repro, { target, props });
    flushSync();

    // User sorts by avg_price (present in the full column set).
    (comp as any).setSort('avg_price');
    flushSync();
    expect((comp as any).getSortKey()).toBe('avg_price');

    // Switch to the `sets` preset whose columns do NOT include avg_price.
    // This is the exact trigger described in the finding.
    props.visibleColumns = ['name', 'owned', 'sell_score', 'low_sell', 'top_buy', 'potential_plat'];

    let threw: unknown = null;
    try {
      flushSync();
    } catch (e) {
      threw = e;
    }

    // The effect should converge: sortKey falls back to the first right-aligned
    // sortable column in the new set ('owned'), with no thrown error and no
    // effect_update_depth_exceeded logged.
    expect(threw).toBeNull();
    const fallback = (comp as any).getSortKey();
    expect(fallback).toBe('owned');

    const depthErr = errors.flat().some(
      (x) => typeof x === 'string' && x.includes('effect_update_depth_exceeded'),
    ) || errors.flat().some(
      (x) => x instanceof Error && String(x.message).includes('effect_update_depth_exceeded'),
    );
    expect(depthErr).toBe(false);

    unmount(comp);
    errSpy.mockRestore();
  });
});
