import { mount } from 'svelte';
import App from './App.svelte';
// Imported AFTER App so the global stylesheet lands last in the bundle: the
// per-look structural rules in app.css rely on winning specificity ties with
// component-scoped rules.
import './app.css';
import { createStateStore } from './lib/state-store';
import { initTheme } from './lib/theme';

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
