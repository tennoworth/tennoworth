<script lang="ts">
  import { tick } from 'svelte';
  import type { PageWindow } from './paging';

  let { window: win, total, label, target, onpage }: {
    window: PageWindow;
    total: number;
    /** Names the list for assistive technology, e.g. "Trade history pages". */
    label: string;
    /** The paged list; its top comes back into view after a page change. */
    target?: HTMLElement;
    onpage: (page: number) => void;
  } = $props();

  // The pager sits under up to 100 rows, so after Next the new page would
  // otherwise open at its bottom.
  async function go(page: number): Promise<void> {
    onpage(page);
    await tick();
    if (!target) return;
    const viewportTop = target.closest('main')?.getBoundingClientRect().top ?? 0;
    if (target.getBoundingClientRect().top < viewportTop) target.scrollIntoView({ block: 'start' });
  }
</script>

{#if win.pages > 1}
  <nav class="pager" aria-label={label}>
    <span class="range">Showing {(win.start + 1).toLocaleString()}–{win.end.toLocaleString()} of {total.toLocaleString()}</span>
    <div class="controls">
      <button type="button" class="btn" disabled={win.page === 0} onclick={() => go(win.page - 1)}>‹ Prev</button>
      <span class="count" aria-live="polite">Page {win.page + 1} of {win.pages}</span>
      <button type="button" class="btn" disabled={win.page >= win.pages - 1} onclick={() => go(win.page + 1)}>Next ›</button>
    </div>
  </nav>
{/if}

<style>
  .pager {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: var(--s2) var(--s3);
    min-height: var(--bar);
    padding: var(--s1) var(--inset);
    border-top: 1px solid var(--border);
    font-size: var(--text-control);
    color: var(--muted);
  }
  .controls { display: flex; flex-wrap: wrap; align-items: center; gap: var(--s2); }
  .count { font-variant-numeric: tabular-nums; }
</style>
