// Theme = a LOOK (token family + structural CSS, `html[data-look]`) × a MODE
// (light/dark, `html[data-mode]`). app.css keys every token block off those
// two attributes; nothing else in the app reads them.
//
// The attribute carries the RESOLVED mode ('light' | 'dark'), never 'system' —
// so app.css needs one dark block per look and no prefers-color-scheme
// duplicates. Resolving 'system' (and following it live) is this module's job.
// public/theme-boot.js re-implements the same resolution inline for first
// paint; if the rules here change, change them there too.

import type { StateStore } from './state-store';

export type Look = 'yorha' | 'corpus' | 'vitruvian' | 'baseline';
export type Mode = 'light' | 'dark';
export type ModePref = Mode | 'system';

export interface LookInfo {
  id: Look;
  label: string;
  blurb: string;
  /** Which resolved modes have their own token block. A look without a
   *  variant for the resolved mode simply keeps its other block — app.css
   *  only overrides on `[data-mode=dark]`, so a light-only look renders its
   *  light tokens under data-mode=dark and vice versa. */
  modes: readonly Mode[];
}

export const LOOKS: readonly LookInfo[] = [
  { id: 'yorha', label: 'YoRHa', blurb: 'Paper & ink, dotted rules', modes: ['light', 'dark'] },
  { id: 'corpus', label: 'Corpus', blurb: 'Ledger terminal, chamfers', modes: ['light'] },
  { id: 'vitruvian', label: 'Vitruvian', blurb: 'Parchment, drafting ticks', modes: ['light'] },
  { id: 'baseline', label: 'Baseline', blurb: 'The original dark dashboard', modes: ['dark'] },
];

export const DEFAULT_LOOK: Look = 'yorha';
export const DEFAULT_MODE_PREF: ModePref = 'system';

const LOOK_IDS = new Set<string>(LOOKS.map((l) => l.id));

export function isLook(v: string | null | undefined): v is Look {
  return !!v && LOOK_IDS.has(v);
}
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
export function applyTheme(look: Look, mode: Mode): void {
  const el = document.documentElement;
  if (el.dataset.look !== look) el.dataset.look = look;
  if (el.dataset.mode !== mode) el.dataset.mode = mode;
}

/** The persisted preference pair, validated — unknown/absent → defaults. */
export function readThemePrefs(store: StateStore): { look: Look; mode: ModePref } {
  const look = store.getSetting('theme.look');
  const mode = store.getSetting('theme.mode');
  return {
    look: isLook(look) ? look : DEFAULT_LOOK,
    mode: isModePref(mode) ? mode : DEFAULT_MODE_PREF,
  };
}

/**
 * Apply the stored preference and keep following the OS while the pref is
 * 'system'. Returns a small controller the switcher drives; `set` persists
 * through the store (never raw localStorage — the desktop build backs it with
 * SQLite). One instance per app; call once at boot after `store.hydrate()`.
 */
export function initTheme(store: StateStore) {
  let { look, mode: pref } = readThemePrefs(store);
  const mq = typeof matchMedia === 'function' ? matchMedia(DARK_MQ) : null;

  const paint = () => applyTheme(look, resolveMode(pref));
  const onSystemChange = () => {
    if (pref === 'system') paint();
  };
  mq?.addEventListener('change', onSystemChange);
  paint();

  return {
    get look() {
      return look;
    },
    get modePref() {
      return pref;
    },
    setLook(next: Look) {
      look = next;
      paint();
      void store.setSetting('theme.look', next);
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
