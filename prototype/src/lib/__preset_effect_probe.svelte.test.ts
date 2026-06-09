import { describe, it, expect } from 'vitest';
import { flushSync } from 'svelte';

// Reproduce the exact reactive shape of App.svelte's preset effect
// (lines 111-177) in a .svelte.ts module so runes are compiled.
// Goal: prove the read+write of `activePreset` reaches a fixed point and
// does NOT infinite-loop ("Maximum update depth exceeded").

interface Preset {
  minPrice: number;
  hideAtLvl: number;
  typeFilter: string;
  activeTags: string[];
}
const PRESETS: Record<string, Preset> = {
  default: { minPrice: 5, hideAtLvl: 5, typeFilter: 'all', activeTags: [] },
  ducats: { minPrice: 0, hideAtLvl: 11, typeFilter: 'all', activeTags: ['prime'] },
};

function makeHarness() {
  let minPrice = $state(5);
  let minOwned = $state(1);
  let hideAtLvl = $state(5);
  let typeFilter = $state('all');
  let activeTags = $state<Set<string>>(new Set());
  let activePreset = $state<string | null>('default');

  let runCount = 0;

  const cleanup = $effect.root(() => {
    $effect(() => {
      runCount++;
      void minPrice; void minOwned; void hideAtLvl; void typeFilter; void activeTags.size;
      if (activePreset === null) return;
      const p = PRESETS[activePreset];
      if (!p) return;
      const matches =
        minPrice === p.minPrice &&
        hideAtLvl === p.hideAtLvl &&
        typeFilter === p.typeFilter &&
        activeTags.size === p.activeTags.length &&
        p.activeTags.every((t) => activeTags.has(t));
      if (!matches) activePreset = null;
    });
  });

  function applyPreset(name: string) {
    const p = PRESETS[name];
    if (!p) return;
    minPrice = p.minPrice;
    hideAtLvl = p.hideAtLvl;
    typeFilter = p.typeFilter;
    activeTags = new Set(p.activeTags);
    activePreset = name;
  }

  return {
    applyPreset,
    bumpMinPrice: () => { minPrice = minPrice + 1; },
    getActivePreset: () => activePreset,
    getRunCount: () => runCount,
    resetRunCount: () => { runCount = 0; },
    snapshot: () => ({ minPrice, hideAtLvl, typeFilter, tags: [...activeTags], activePreset }),
    cleanup,
  };
}

describe('App.svelte preset $effect — reachability of the claimed loop', () => {
  it('applyPreset does NOT null the preset and does not loop', () => {
    const h = makeHarness();
    flushSync();
    expect(h.getActivePreset()).toBe('default'); // sanity: initial state
    h.resetRunCount();

    // Scenario A from the finding: apply a preset.
    flushSync(() => h.applyPreset('ducats'));
    flushSync();

    console.log('[probe] after applyPreset(ducats):', JSON.stringify(h.snapshot()), 'runs=', h.getRunCount());

    // The finding claims filters "don't yet match" so it nulls activePreset.
    // Reality: applyPreset writes filters to exactly the preset's values.
    expect(h.getActivePreset()).toBe('ducats');
    // Bounded re-fires (no runaway). One write batch + the self-settle pass.
    expect(h.getRunCount()).toBeLessThan(5);
    h.cleanup();
  });

  it('manual edit while a preset is active nulls it and SETTLES (no infinite loop)', () => {
    const h = makeHarness();
    flushSync(() => h.applyPreset('ducats'));
    flushSync();
    expect(h.getActivePreset()).toBe('ducats');
    h.resetRunCount();

    // Path 2: user bumps minPrice -> diverges from preset.
    flushSync(() => h.bumpMinPrice());
    flushSync();

    // The effect writes activePreset=null, re-fires once, hits the early
    // return, and stops. If it looped, flushSync would throw
    // "Maximum update depth exceeded".
    expect(h.getActivePreset()).toBe(null);
    expect(h.getRunCount()).toBeLessThan(6);
    h.cleanup();
  });
});
