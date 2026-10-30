<script lang="ts">
  import { onMount } from 'svelte';
  import { datedHistory } from './model';
  import { tableNavigation } from '../../ui/keyboard.svelte';
  import { pageWindow } from '../../ui/paging';
  import Pager from '../../ui/Pager.svelte';
  let { slug, load }: { slug: string; load: () => Promise<unknown> } = $props();
  let history = $state<ReturnType<typeof datedHistory>>(null);
  let historyState = $state<'idle' | 'loading' | 'ready' | 'unavailable'>('idle');
  let page = $state(0);
  let enhanced = $state(false);
  onMount(() => { enhanced = true; });
  let target = $state<HTMLDivElement>();
  let win = $derived(pageWindow(history?.rows.length ?? 0, page));
  async function fetchHistory() {
    if (historyState === 'loading') return;
    historyState = 'loading';
    try {
      history = datedHistory(await load(), slug);
      historyState = history ? 'ready' : 'unavailable';
      page = 0;
    } catch { historyState = 'unavailable'; }
  }
</script>
<section aria-label="Dated price history">
  <h2>Dated price history</h2>
  <p>Load the existing year-long public history when you need exact dates. This is a separate source window from the current price snapshot. Missing observations remain unavailable; they are not zero-price trades.</p>
  {#if historyState !== 'ready'}<button class="btn" disabled={!enhanced || historyState === 'loading'} onclick={fetchHistory}>{historyState === 'loading' ? 'Loading history…' : historyState === 'unavailable' ? 'Retry dated history' : 'Load dated history'}</button>{/if}
  <p>The dated table requires JavaScript. The snapshot comparison above remains available without it.</p>
  {#if historyState === 'loading'}<p role="status">Loading the public history file…</p>{/if}
  {#if historyState === 'unavailable'}<p role="status">Dated history is unavailable or cannot be matched to this item. The snapshot comparison is unchanged.</p>{/if}
  {#if history && historyState === 'ready'}
    <p>Source: relics.run via TennoWorth’s public history export. Through {history.through}; generated {history.generated}. Newest dates first. A missing median can mean no reported trades or missing source data; volume is shown separately when known.</p>
    <div class="public-table" bind:this={target} use:tableNavigation={'Dated price history'}><table class="tw"><thead><tr><th>Date (UTC)</th><th>Daily median</th><th>Reported volume</th></tr></thead><tbody>
      {#each history.rows.slice(win.start, win.end) as row}<tr><td>{row.date}</td><td>{row.median == null ? 'Unavailable' : `${row.median} p`}</td><td>{row.volume ?? 'Unavailable'}</td></tr>{/each}
    </tbody></table></div>
    <Pager window={win} total={history.rows.length} label="Price history pages" {target} onpage={next => page = next} />
  {/if}
</section>
