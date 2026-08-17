import { describe, it, expect, beforeEach, vi } from 'vitest';
import { LocalStorageStateStore, LOCAL_SETTING_KEYS } from './state-store';
import { initTheme, readThemePrefs, resolveMode, LOOKS, DEFAULT_LOOK } from './theme';
// The pre-paint boot script, as text: it must know the same ids/keys the app
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
  it('falls back to the defaults for missing or unknown values', () => {
    const store = new LocalStorageStateStore();
    expect(readThemePrefs(store)).toEqual({ look: DEFAULT_LOOK, mode: 'system' });
    localStorage.setItem(LOCAL_SETTING_KEYS['theme.look'], 'not-a-look');
    localStorage.setItem(LOCAL_SETTING_KEYS['theme.mode'], 'sepia');
    expect(readThemePrefs(store)).toEqual({ look: DEFAULT_LOOK, mode: 'system' });
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
  it('stamps <html> from the store and persists changes through it', async () => {
    stubMatchMedia(false);
    localStorage.setItem(LOCAL_SETTING_KEYS['theme.look'], 'corpus');
    const store = new LocalStorageStateStore();
    const t = initTheme(store);
    expect(document.documentElement.dataset.look).toBe('corpus');
    expect(document.documentElement.dataset.mode).toBe('light');
    t.setModePref('dark');
    t.setLook('vitruvian');
    expect(document.documentElement.dataset).toMatchObject({ look: 'vitruvian', mode: 'dark' });
    expect(localStorage.getItem(LOCAL_SETTING_KEYS['theme.look'])).toBe('vitruvian');
    expect(localStorage.getItem(LOCAL_SETTING_KEYS['theme.mode'])).toBe('dark');
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

  it('every look declares at least one mode and the boot script knows the same ids', () => {
    const boot = bootScript;
    for (const l of LOOKS) {
      expect(l.modes.length).toBeGreaterThan(0);
      // The pre-paint script must accept exactly the ids the app persists.
      expect(boot).toContain(`'${l.id}'`);
    }
    expect(boot).toContain(LOCAL_SETTING_KEYS['theme.look']);
    expect(boot).toContain(LOCAL_SETTING_KEYS['theme.mode']);
  });
});
