import { describe, it, expect, vi, beforeEach } from 'vitest';
import { LocalStorageStateStore, LOCAL_SETTING_KEYS } from '../../adapters/state-store';
import type { Mode } from '../../ui/theme';
import { AppIconController, resolveAppIcon, readAppIconChoice } from './app-icon.svelte';

function fakeTheme(initial: Mode) {
  const listeners = new Set<(mode: Mode) => void>();
  const theme = {
    mode: initial,
    subscribe(fn: (mode: Mode) => void) {
      listeners.add(fn);
      return () => listeners.delete(fn);
    },
  };
  return {
    theme,
    change(mode: Mode) {
      theme.mode = mode;
      listeners.forEach((fn) => fn(mode));
    },
  };
}

const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

beforeEach(() => localStorage.clear());

describe('app icon choice', () => {
  it('match resolves to the colour mode’s own ink; the others are themselves', () => {
    expect(resolveAppIcon('match', 'light')).toBe('ink');
    expect(resolveAppIcon('match', 'dark')).toBe('rag');
    for (const mode of ['light', 'dark'] as const) {
      expect(resolveAppIcon('blue', mode)).toBe('blue');
      expect(resolveAppIcon('ink', mode)).toBe('ink');
      expect(resolveAppIcon('rag', mode)).toBe('rag');
    }
  });

  it('defaults to classic blue for a missing or unknown stored value', () => {
    const store = new LocalStorageStateStore();
    expect(readAppIconChoice(store)).toBe('blue');
    localStorage.setItem(LOCAL_SETTING_KEYS['app-icon'], 'teal');
    expect(readAppIconChoice(store)).toBe('blue');
    localStorage.setItem(LOCAL_SETTING_KEYS['app-icon'], 'rag');
    expect(readAppIconChoice(store)).toBe('rag');
  });
});

describe('AppIconController', () => {
  it('applies the stored choice at start and re-applies match when the mode changes', async () => {
    localStorage.setItem(LOCAL_SETTING_KEYS['app-icon'], 'match');
    const { theme, change } = fakeTheme('light');
    const setAppIcon = vi.fn(async (_colour: string) => ({ windowIcon: true }));
    const icon = new AppIconController({ store: new LocalStorageStateStore(), theme, native: { setAppIcon } });
    const stop = icon.start();
    await settle();
    change('dark');
    await settle();
    change('light');
    await settle();
    stop();
    change('dark');
    await settle();
    expect(setAppIcon.mock.calls.map(([colour]) => colour)).toEqual(['ink', 'rag', 'ink']);
  });

  it('does not call native again when a mode change leaves the colour as it is', async () => {
    const { theme, change } = fakeTheme('light');
    const setAppIcon = vi.fn(async (_colour: string) => ({ windowIcon: true }));
    const icon = new AppIconController({ store: new LocalStorageStateStore(), theme, native: { setAppIcon } });
    icon.start();
    await settle();
    change('dark');
    await settle();
    expect(setAppIcon.mock.calls).toEqual([['blue']]);
  });

  it('persists a new choice and applies its resolved colour', async () => {
    const { theme } = fakeTheme('dark');
    const setAppIcon = vi.fn(async (_colour: string) => ({ windowIcon: false }));
    const icon = new AppIconController({ store: new LocalStorageStateStore(), theme, native: { setAppIcon } });
    await icon.select('match');
    expect(localStorage.getItem(LOCAL_SETTING_KEYS['app-icon'])).toBe('match');
    expect(setAppIcon).toHaveBeenLastCalledWith('rag');
    expect(icon.windowIcon).toBe(false);
  });

  it('reports a native failure and retries the same colour next time', async () => {
    const { theme } = fakeTheme('light');
    const setAppIcon = vi.fn()
      .mockRejectedValueOnce(new Error('tray unavailable'))
      .mockResolvedValue({ windowIcon: true });
    const icon = new AppIconController({ store: new LocalStorageStateStore(), theme, native: { setAppIcon } });
    await icon.select('ink');
    expect(icon.error).toBe('tray unavailable');
    await icon.select('blue');
    await icon.select('ink');
    expect(icon.error).toBe('');
    expect(setAppIcon.mock.calls.map(([colour]) => colour)).toEqual(['ink', 'blue', 'ink']);
  });
});
