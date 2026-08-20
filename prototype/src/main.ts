import { mount } from 'svelte';
import App from './App.svelte';
// Imported AFTER App so the global stylesheet lands last in the bundle: the
// per-look structural rules in app.css rely on winning specificity ties with
// component-scoped rules.
import './app.css';
import { createStateStore } from './lib/state-store';
import { initTheme } from './lib/theme';

// Dev-only design-review seam: `?preview-desktop` on a `vite dev` origin
// installs a stub Tauri runtime BEFORE anything sniffs for it, so the
// desktop-gated views (Rivens, My orders, Watches, Ledger) can be eyeballed
// in a plain browser with a seeded snapshot. Every IPC command resolves to a
// safe empty. Dead code in production: the whole block is tree-shaken out of
// `vite build` (import.meta.env.DEV is false), and the desktop webview ships
// its real runtime long before this line runs.
if (import.meta.env.DEV && new URLSearchParams(location.search).has('preview-desktop')) {
  const empties: Record<string, unknown> = {
    wfm_auth_status: { logged_in: false, unlocked: false },
    tray_state: { labels: [], last_notification: null },
    refresh_history: { updated: false, body: null },
    refresh_market: { updated: false, status: 'offline' },
    top_sellables: [], list_watches: [], list_listing_log: [], list_snapshots: [],
    ledger_rows: [], list_trades: [],
    try_silent_unlock: false,
  };
  // The desktop store keeps settings + the reload-restore snapshot in SQLite
  // via get_setting/set_setting; back those onto localStorage so a seeded
  // browser snapshot round-trips exactly like the real thing.
  const invoke = (cmd: string, args?: Record<string, unknown>) => {
    if (cmd === 'get_setting') return Promise.resolve(localStorage.getItem(String(args?.key)));
    if (cmd === 'set_setting') { localStorage.setItem(String(args?.key), String(args?.value)); return Promise.resolve(null); }
    if (cmd === 'delete_setting') { localStorage.removeItem(String(args?.key)); return Promise.resolve(null); }
    return Promise.resolve(cmd in empties ? empties[cmd] : null);
  };
  const w = globalThis as Record<string, unknown>;
  w.__TAURI_INTERNALS__ = { invoke };
  w.__TAURI__ = { core: { invoke }, event: { listen: () => Promise.resolve(() => {}) } };
}

const target = document.getElementById('app');
if (!target) throw new Error('#app mount target missing in index.html');

// Pick the persistence backend (localStorage vs SQLite-over-IPC) and prime its
// scalar-settings cache BEFORE mounting, so App can read them synchronously at
// component init with no default-value flash — in the browser and the desktop
// build alike. hydrate() never rejects; if it somehow did we still mount rather
// than leave a blank window.
const store = createStateStore();
const app = store.hydrate().then(() => {
  // public/theme-boot.js already stamped the browser's stored theme before
  // first paint; this re-applies from the store (the desktop build keeps
  // settings in SQLite, which the boot script can't see) and starts following
  // the OS scheme. Before mount, so the theme never changes under the UI.
  const theme = initTheme(store);
  return mount(App, { target, props: { store, theme } });
});
export default app;
