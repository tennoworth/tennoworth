import { mount } from 'svelte';
import App from './App.svelte';
import './app.css';
import { createStateStore } from './lib/state-store';

const target = document.getElementById('app');
if (!target) throw new Error('#app mount target missing in index.html');

// Pick the persistence backend (localStorage vs SQLite-over-IPC) and prime its
// scalar-settings cache BEFORE mounting, so App can read them synchronously at
// component init with no default-value flash — in the browser and the desktop
// build alike. hydrate() never rejects; if it somehow did we still mount rather
// than leave a blank window.
const store = createStateStore();
const app = store.hydrate().then(() => mount(App, { target, props: { store } }));
export default app;
