import { mount } from 'svelte';
import '../app.css';
import AdminApp from './AdminApp.svelte';
import { boxApi, publisher } from './api';
import { LocalStorageStateStore } from '../adapters/state-store';
import { initTheme } from '../ui/theme';

const target = document.getElementById('app');
if (target) mount(AdminApp, { target, props: { publisher, boxApi, theme: initTheme(new LocalStorageStateStore()) } });
