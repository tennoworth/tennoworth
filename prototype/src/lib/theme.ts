// Theme = one LOOK (yorha - token family + structural CSS, `html[data-look]`)
// × a MODE (light/dark, `html[data-mode]`). app.css keys every token block off
// those two attributes; nothing else in the app reads them.
//
// There used to be four looks and a picker. Since 2026-08 yorha is THE theme,
// so the user's only choice is the mode; `data-look="yorha"` is still stamped
// because a large amount of structural CSS is scoped by it.
//
// The mode attribute carries the RESOLVED mode ('light' | 'dark'), never
// 'system' - so app.css needs one dark block and no prefers-color-scheme
// duplicates. Resolving 'system' (and following it live) is this module's job.
// public/theme-boot.js re-implements the same resolution inline for first
// paint; if the rules here change, change them there too.

import type { StateStore } from './state-store';

/** The one look. Stamped on <html> for the structural CSS to hang off. */
export const LOOK = 'yorha';

export type Mode = 'light' | 'dark';
export type ModePref = Mode | 'system';

export const DEFAULT_MODE_PREF: ModePref = 'system';

export function isModePref(v: string | null | undefined): v is ModePref {
  return v === 'light' || v === 'dark' || v === 'system';
}

const DARK_MQ = '(prefers-color-scheme: dark)';

export function systemMode(): Mode {
  return typeof matchMedia === 'function' && matchMedia(DARK_MQ).matches ? 'dark' : 'light';
}

export function resolveMode(pref: ModePref): Mode {
  return pref === 'system' ? systemMode() : pref;
}

/** Stamp the attributes app.css keys its tokens off. Idempotent. */
export function applyTheme(mode: Mode): void {
  const el = document.documentElement;
  if (el.dataset.look !== LOOK) el.dataset.look = LOOK;
  if (el.dataset.mode !== mode) el.dataset.mode = mode;
}

/** The persisted mode preference, validated - unknown/absent → the default. */
export function readThemePref(store: StateStore): ModePref {
  const mode = store.getSetting('theme.mode');
  return isModePref(mode) ? mode : DEFAULT_MODE_PREF;
}

/**
 * Apply the stored preference and keep following the OS while the pref is
 * 'system'. Returns a small controller the switcher drives; `setModePref`
 * persists through the store (never raw localStorage - the desktop build backs
 * it with SQLite). One instance per app; call once at boot after
 * `store.hydrate()`.
 */
export function initTheme(store: StateStore) {
  let pref = readThemePref(store);
  const mq = typeof matchMedia === 'function' ? matchMedia(DARK_MQ) : null;

  const paint = () => applyTheme(resolveMode(pref));
  const onSystemChange = () => {
    if (pref === 'system') paint();
  };
  mq?.addEventListener('change', onSystemChange);
  paint();

  return {
    get modePref() {
      return pref;
    },
    setModePref(next: ModePref) {
      pref = next;
      paint();
      void store.setSetting('theme.mode', next);
    },
    destroy() {
      mq?.removeEventListener('change', onSystemChange);
    },
  };
}

export type ThemeController = ReturnType<typeof initTheme>;
