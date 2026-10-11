import { mount } from 'svelte';
import '../app.css';
import Dashboard from './Dashboard.svelte';
import { publisher } from './api';
import { LocalStorageStateStore } from '../adapters/state-store';
import { initTheme } from '../ui/theme';

const target = document.getElementById('app');
if (target) mount(Dashboard, { target, props: { publisher, theme: initTheme(new LocalStorageStateStore()) } });
