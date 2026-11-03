/**
 * How item lists draw: Visual shows item art in roomier rows, Compact keeps
 * dense text rows. The choice is stamped on <html> as `data-density` so the
 * shared stylesheet does the work; components only need this for the toggle.
 */
import { getContext, setContext } from 'svelte';
import type { SettingsStore } from '../contracts/state-store';

export type Display = 'visual' | 'compact';

export function isDisplay(value: string | null | undefined): value is Display {
  return value === 'visual' || value === 'compact';
}

export class DisplayController {
  value = $state<Display>('visual');

  constructor(private store: SettingsStore) {
    const saved = store.getSetting('display');
    this.value = isDisplay(saved) ? saved : 'visual';
    this.#stamp();
  }

  set(next: Display): void {
    if (next === this.value) return;
    this.value = next;
    this.#stamp();
    void this.store.setSetting('display', next);
  }

  #stamp(): void {
    document.documentElement.dataset.density = this.value;
  }
}

const KEY = Symbol('display');

export function provideDisplay(display: DisplayController): void {
  setContext(KEY, display);
}

/** Absent outside the desktop shell, where lists keep their compact form. */
export function useDisplay(): DisplayController | undefined {
  return getContext<DisplayController | undefined>(KEY);
}
