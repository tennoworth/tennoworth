import { DESKTOP_CONTEXT } from './contracts/services';
import { mount, hydrate } from 'svelte';
// Shared theme rules follow shell layout rules so existing specificity ties
// retain the production cascade.
import './shell.css';
import './app.css';
import { createStateStore } from './adapters/state-store';
import { initTheme } from './ui/theme';
import { installDesktopExternalLinkHandler } from './adapters/runtime';

// Dev-only design-review seam: `?preview-desktop` on a `vite dev` origin
// installs a stub Tauri runtime BEFORE anything sniffs for it, so the
// desktop-gated views (Rivens, My orders, Watches, Ledger) can be eyeballed
// in a plain browser with a seeded snapshot. The optional sample scenario
// supplies populated or failure responses. The block is tree-shaken out of
// `vite build` (import.meta.env.DEV is false), and the desktop webview ships
// its real runtime long before this line runs.
if (import.meta.env.DEV && new URLSearchParams(location.search).has('preview-desktop')) {
  await (await import('./dev/preview/install-preview')).installPreview();
}

const target = document.getElementById('app');
if (!target) throw new Error('#app mount target missing in index.html');
const publicPayload = document.getElementById('public-page-data');
installDesktopExternalLinkHandler();

// Pick the persistence backend (localStorage vs SQLite-over-IPC) and prime its
// scalar-settings cache BEFORE mounting, so App can read them synchronously at
// component init with no default-value flash - in the browser and the desktop
// build alike. hydrate() never rejects; if it somehow did we still mount rather
// than leave a blank window.
const overlaySurface = new URLSearchParams(location.search).get('surface') === 'relic-overlay';
document.documentElement.classList.toggle('relic-overlay-surface', overlaySurface);
const store = createStateStore();
// Keep the overlay's transparent, scroll-locked document CSS out of the hosted
// page's initial bundle. A conditional mount does not isolate a static import's
// component CSS: Svelte still emits it globally even when that branch never runs.
const styleguideSurface = import.meta.env.DEV && new URLSearchParams(location.search).has('styleguide');
const app = styleguideSurface && !overlaySurface
  ? import('./dev/styleguide/Styleguide.svelte').then(({ default: Styleguide }) => mount(Styleguide, { target }))
  : overlaySurface
  ? Promise.all([import('./desktop/relics/RelicOverlay.svelte'), import('./adapters/services')]).then(([{ default: RelicOverlay }, { createDesktopServices }]) => mount(RelicOverlay, { target, context: new Map([[DESKTOP_CONTEXT, createDesktopServices()]]) }))
  : store.hydrate().then(() => {
    // public/theme-boot.js already stamped the browser's stored theme before
    // first paint; this re-applies from the store (the desktop build keeps
    // settings in SQLite, which the boot script can't see) and starts following
    // the OS scheme. Before mount, so the theme never changes under the UI.
    const theme = initTheme(store);
    // createStateStore picked the store by the same runtime sniff, so its mode
    // decides the shell, and only the desktop store carries a snapshot.
    if (publicPayload && store.mode !== 'tauri') {
      return import('./site/shell/PublicPage.svelte').then(({ default: PublicPage }) => hydrate(PublicPage, { target, props: { ...JSON.parse(publicPayload.textContent ?? '{}'), theme } }));
    }
    // mount() appends to #app instead of replacing its children. The hosted
    // build prerenders content there, so drop it only once the shell chunk has
    // loaded: a chunk that fails to load must leave the readable page intact.
    return store.mode === 'tauri'
      ? Promise.all([import('./desktop/shell/DesktopShell.svelte'), import('./adapters/services')]).then(([{ default: DesktopShell }, { createDesktopServices }]) => {
          target.replaceChildren();
          return mount(DesktopShell, { target, props: { store, theme }, context: new Map([[DESKTOP_CONTEXT, createDesktopServices()]]) });
        })
      : import('./site/shell/HostedShell.svelte').then(({ default: HostedShell }) => {
          target.replaceChildren();
          return mount(HostedShell, { target, props: { theme, store } });
        });
  });
export default app;
