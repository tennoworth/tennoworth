<script lang="ts">
  import { onMount } from 'svelte';
  import Dashboard from './Dashboard.svelte';
  import StatusPage from './StatusPage.svelte';
  import type { BoxApi, Publisher } from './api';
  import type { ThemeController } from '../ui/theme';

  let { publisher, boxApi, theme }: { publisher: Publisher; boxApi: BoxApi; theme: ThemeController } = $props();
  type Page = 'status' | 'announcements';
  function fromHash(): Page { return location.hash === '#announcements' ? 'announcements' : 'status'; }
  let page = $state<Page>(fromHash());
  onMount(() => {
    const sync = () => { page = fromHash(); };
    window.addEventListener('hashchange', sync);
    return () => window.removeEventListener('hashchange', sync);
  });
</script>

<nav class="admin-nav" aria-label="Dashboard pages">
  <span class="brand">TennoWorth admin</span>
  <div class="ui-segmented">
    <a href="#status" aria-current={page === 'status' ? 'page' : undefined}>Status</a>
    <a href="#announcements" aria-current={page === 'announcements' ? 'page' : undefined}>Announcements</a>
  </div>
</nav>
<!-- Both pages stay mounted, so a draft in progress survives a look at the box. -->
<main class="admin-status" hidden={page !== 'status'}><StatusPage api={boxApi} active={page === 'status'} /></main>
<div hidden={page !== 'announcements'}><Dashboard {publisher} {theme} /></div>

<style>
  .admin-nav { width: min(100%, 76rem); margin: 0 auto; padding: var(--s4) var(--s4) 0; display: flex; flex-wrap: wrap; align-items: center; gap: var(--s2) var(--s4); }
  .brand { font: 600 var(--text-section)/1 var(--font-ui); letter-spacing: .04em; }
  .admin-nav .ui-segmented > a[aria-current="page"] { background: var(--ink-bar); color: var(--on-ink); font-weight: 600; }
  .admin-status { width: min(100%, 76rem); margin: 0 auto; padding: var(--s4); }
</style>
