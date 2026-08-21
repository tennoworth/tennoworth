import { describe, it, expect, beforeEach, vi } from 'vitest';
import { LocalStorageStateStore, LOCAL_SETTING_KEYS } from './state-store';
import { initTheme, readThemePref, resolveMode, LOOK, DEFAULT_MODE_PREF } from './theme';
// The pre-paint boot script, as text: it must know the same key the app
// persists, and nothing else can pin that (it's a classic script, not a module).
import bootScript from '../../public/theme-boot.js?raw';

// jsdom has no matchMedia; a stub lets the tests drive "system" resolution.
function stubMatchMedia(dark: boolean) {
  const listeners = new Set<() => void>();
  const mql = {
    matches: dark,
    addEventListener: (_: string, fn: () => void) => listeners.add(fn),
    removeEventListener: (_: string, fn: () => void) => listeners.delete(fn),
  };
  vi.stubGlobal('matchMedia', () => mql);
  return {
    flip(next: boolean) {
      mql.matches = next;
      listeners.forEach((fn) => fn());
    },
  };
}

beforeEach(() => {
  localStorage.clear();
  delete document.documentElement.dataset.look;
  delete document.documentElement.dataset.mode;
});

describe('theme prefs', () => {
  it('falls back to the default for a missing or unknown value', () => {
    const store = new LocalStorageStateStore();
    expect(readThemePref(store)).toBe(DEFAULT_MODE_PREF);
    localStorage.setItem(LOCAL_SETTING_KEYS['theme.mode'], 'sepia');
    expect(readThemePref(store)).toBe(DEFAULT_MODE_PREF);
  });

  it('resolves system to the OS scheme, pins otherwise', () => {
    stubMatchMedia(true);
    expect(resolveMode('system')).toBe('dark');
    expect(resolveMode('light')).toBe('light');
    stubMatchMedia(false);
    expect(resolveMode('system')).toBe('light');
  });
});

describe('initTheme', () => {
  it('stamps <html> from the store and persists changes through it', () => {
    stubMatchMedia(false);
    const store = new LocalStorageStateStore();
    const t = initTheme(store);
    expect(document.documentElement.dataset.look).toBe(LOOK);
    expect(document.documentElement.dataset.mode).toBe('light');
    t.setModePref('dark');
    expect(document.documentElement.dataset).toMatchObject({ look: LOOK, mode: 'dark' });
    expect(t.modePref).toBe('dark');
    expect(localStorage.getItem(LOCAL_SETTING_KEYS['theme.mode'])).toBe('dark');
    t.destroy();
  });

  // The four-look era persisted `wfminv:theme-look-v1`. Nothing reads it now;
  // a returning user carrying 'corpus' must simply get yorha, never a throw.
  it('ignores a stale look preference from the retired picker', () => {
    stubMatchMedia(false);
    localStorage.setItem('wfminv:theme-look-v1', 'corpus');
    const store = new LocalStorageStateStore();
    const t = initTheme(store);
    expect(document.documentElement.dataset.look).toBe(LOOK);
    expect(Object.keys(LOCAL_SETTING_KEYS)).not.toContain('theme.look');
    t.destroy();
  });

  it('follows the OS scheme only while the pref is system', () => {
    const mq = stubMatchMedia(false);
    const t = initTheme(new LocalStorageStateStore());
    expect(document.documentElement.dataset.mode).toBe('light');
    mq.flip(true);
    expect(document.documentElement.dataset.mode).toBe('dark');
    t.setModePref('light');
    mq.flip(false);
    mq.flip(true);
    expect(document.documentElement.dataset.mode).toBe('light');
    t.destroy();
  });

  it('the boot script stamps the same look and reads the same key', () => {
    expect(bootScript).toContain(`'${LOOK}'`);
    expect(bootScript).toContain(LOCAL_SETTING_KEYS['theme.mode']);
    // …and no longer looks a look up.
    expect(bootScript).not.toContain(`getItem('wfminv:theme-look-v1')`);
  });
});
