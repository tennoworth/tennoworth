<script lang="ts">
  import { useDesktopServices } from '../../ui/desktop-context';
  const { desktopAddWatch, desktopCheckWatchesNow, desktopDeleteWatch, desktopListWatches, listenForTauriEvent } = useDesktopServices();
  import { onMount } from 'svelte';
  import { DesktopCmdError } from '../../contracts/errors';

import { WATCH_FIRED_EVENT } from '../../contracts/events';
import { type NewWatch, type Watch, type WatchOutcome } from '../../contracts/desktop';
  
  import { buildBrowseIndex, searchItems, type BrowseRow } from '../../domain/market-browse';
  import { humanError } from '../../contracts/errors';
  import { plat } from '../../ui/format';
  import type { Market } from '../../contracts/data';
  import Toast from '../../ui/Toast.svelte';

  interface Props {
    /** Price snapshot - powers the item search and the "now ~Xp" hint. */
    market?: Market | null;
  }
  let { market = null }: Props = $props();

  // ---- state ----
  let watches = $state<Watch[]>([]);
  let loadError = $state<string | null>(null);
  let checking = $state(false);
  let lastOutcomes = $state<Map<number, WatchOutcome>>(new Map());
  let busyIds = $state<Set<number>>(new Set());

  // ---- add form ----
  let query = $state('');
  let picked = $state<BrowseRow | null>(null);
  let side = $state<'sell' | 'buy'>('sell');
  let threshold = $state<number>(0);
  let adding = $state(false);

  let index = $derived(buildBrowseIndex(market));
  let results = $derived(picked ? [] : searchItems(market, index, query, 8));

  // Toasts, component-local (same shape as MyOrdersPanel).
  interface ToastMsg { id: number; kind: 'error' | 'success'; text: string }
  let toasts = $state<ToastMsg[]>([]);
  let toastSeq = 0;
  function pushToast(text: string, kind: 'error' | 'success' = 'success'): void {
    const id = ++toastSeq;
    toasts = [...toasts, { id, kind, text }];
    window.setTimeout(() => (toasts = toasts.filter((t) => t.id !== id)), 4500);
  }

  async function load(): Promise<void> {
    try {
      watches = await desktopListWatches();
      loadError = null;
    } catch (e) {
      loadError = e instanceof DesktopCmdError ? e.message : humanError(e);
    }
  }

  onMount(() => {
    void load();
    return listenForTauriEvent<WatchOutcome>(WATCH_FIRED_EVENT, (o) => {
      const next = new Map(lastOutcomes);
      next.set(o.id, o);
      lastOutcomes = next;
      pushToast(describe(o));
      void load();
    });
  });

  function pick(r: BrowseRow): void {
    picked = r;
    query = r.name;
    // Sensible default: a "buy cheap" watch 20% under the current avg, or a
    // "sell dear" watch 20% over it. The user edits before saving.
    threshold = Math.max(1, Math.round(side === 'sell' ? r.avg * 0.8 : r.avg * 1.2));
  }
  function clearPick(): void {
    picked = null;
    query = '';
    threshold = 0;
  }
  function onSideChange(): void {
    if (picked) threshold = Math.max(1, Math.round(side === 'sell' ? picked.avg * 0.8 : picked.avg * 1.2));
  }

  async function add(): Promise<void> {
    if (!picked || threshold < 1) return;
    adding = true;
    try {
      const w: NewWatch = { slug: picked.slug, name: picked.name, side, threshold, rank: 0, subtype: null };
      watches = await desktopAddWatch(w);
      pushToast(`Watching ${picked.name}: ${side === 'sell' ? '≤' : '≥'} ${threshold}p.`);
      clearPick();
    } catch (e) {
      pushToast(e instanceof DesktopCmdError ? e.message : humanError(e), 'error');
    } finally {
      adding = false;
    }
  }

  async function remove(w: Watch): Promise<void> {
    busyIds = new Set([...busyIds, w.id]);
    try {
      watches = await desktopDeleteWatch(w.id);
    } catch (e) {
      pushToast(e instanceof DesktopCmdError ? e.message : humanError(e), 'error');
    } finally {
      const next = new Set(busyIds); next.delete(w.id); busyIds = next;
    }
  }

  async function checkNow(): Promise<void> {
    if (checking) return;
    checking = true;
    try {
      const out = await desktopCheckWatchesNow();
      const next = new Map<number, WatchOutcome>();
      for (const o of out) next.set(o.id, o);
      lastOutcomes = next;
      const hits = out.filter((o) => o.satisfied).length;
      pushToast(hits ? `${hits} watch${hits === 1 ? '' : 'es'} satisfied right now.` : 'No watch is satisfied right now.');
      await load();
    } catch (e) {
      pushToast(e instanceof DesktopCmdError ? e.message : humanError(e), 'error');
    } finally {
      checking = false;
    }
  }

  function describe(o: WatchOutcome): string {
    if (o.price == null) return `${o.name}: no live data`;
    return o.side === 'sell'
      ? `${o.name} is listed at ${o.price}p (you wanted ≤ ${o.threshold}p)`
      : `Someone bids ${o.price}p for ${o.name} (you wanted ≥ ${o.threshold}p)`;
  }

  function ago(unixSecs: number | null): string {
    if (unixSecs == null) return 'not checked yet';
    const s = Math.max(0, Math.floor(Date.now() / 1000) - unixSecs);
    if (s < 60) return 'just now';
    if (s < 3600) return `${Math.floor(s / 60)} min ago`;
    if (s < 86400) return `${Math.floor(s / 3600)} h ago`;
    return `${Math.floor(s / 86400)} d ago`;
  }

  function status(w: Watch): { label: string; cls: string } {
    if (w.last_price == null) return { label: '-', cls: 'muted' };
    const hit = w.side === 'sell' ? w.last_price <= w.threshold : w.last_price >= w.threshold;
    return hit ? { label: 'satisfied', cls: 'good' } : { label: 'waiting', cls: 'muted' };
  }
</script>

<section class="ui-stack watchlist" data-testid="watchlist">
  <header class="view-header row">
    <div><h2>Price watches</h2><p class="lede">Get a desktop notification when an item reaches your target price.</p></div>
    <div class="row gap-sm">
      <button class="btn ghost" onclick={checkNow} disabled={checking || watches.length === 0}
        title="Run a check right now (the app also checks every 10 minutes in the background and notifies you).">
        {checking ? 'Checking…' : 'Check now'}
      </button>
    </div>
  </header>

  <section class="wrap tw watch-create" aria-labelledby="watch-create-title">
    <div class="rail"><h3 id="watch-create-title">Add a watch</h3></div>
  <div class="add">
    <div class="pick ui-field">
      <label for="watch-item">Item to watch</label>
      <div class="pick-input">
      <input
        id="watch-item"
        class="ui-input"
        type="text"
        placeholder="Item to watch - try “primed flow”, “ash prime set”…"
        bind:value={query}
        oninput={() => { if (picked && query !== picked.name) picked = null; }}
        aria-label="Item to watch"
      />
      {#if picked}
        <button class="btn xs ghost" onclick={clearPick} aria-label="Clear item">×</button>
      {/if}
      </div>
      {#if !picked && results.length}
        <ul class="suggest" role="listbox">
          {#each results as r (r.slug)}
            <li><button type="button" onclick={() => pick(r)}>
              <span class="name">{r.name}</span>
              <span class="muted">{plat(r.avg)}p · {r.vol}/48h</span>
            </button></li>
          {/each}
        </ul>
      {/if}
    </div>
    <label class="ui-field">
      <span>Notify when</span>
      <select class="ui-input" bind:value={side} onchange={onSideChange} aria-label="Watch side">
        <option value="sell">Tell me when the lowest ask is ≤</option>
        <option value="buy">Tell me when the highest bid is ≥</option>
      </select>
    </label>
    <label class="ui-field threshold">
      <span>Target price (p)</span>
      <input class="ui-input" type="number" min="1" bind:value={threshold} aria-label="Threshold (plat)"  disabled={!picked} />
    </label>
    <button class="btn primary" onclick={add} disabled={!picked || threshold < 1 || adding}>Add watch</button>
  </div>

    <div class="foot">Checks online orders every 10 minutes, excluding your own. At most one notification per watch every 6 hours.</div>
  </section>
  <section class="wrap tw" aria-labelledby="watch-list-title">
    <div class="rail"><h3 id="watch-list-title">Your watches</h3><span class="exp">{watches.length} of 100</span></div>
  {#if loadError}
    <div class="ui-notice" data-tone="bad" role="alert">Couldn't load watches: {loadError}</div>
  {:else if watches.length === 0}
    <div class="ui-notice">No watches yet. Pick an item above.</div>
  {:else}
    <div class="scroll">
      <table class="tw fixed">
        <colgroup><col /><col style="width:10rem" /><col style="width:11rem" /><col style="width:8rem" /><col style="width:7rem" /></colgroup>
        <thead><tr><th class="l">Item</th><th>Condition</th><th>Last seen</th><th>Status</th><th>Action</th></tr></thead>
        <tbody>
          {#each watches as w (w.id)}
            {@const st = status(w)}
            {@const o = lastOutcomes.get(w.id)}
            <tr>
              <td class="l">{w.name}{#if w.subtype}<span class="muted"> · {w.subtype}</span>{/if}</td>
              <td class="mono">{w.side === 'sell' ? 'ask ≤' : 'bid ≥'} {w.threshold}p</td>
              <td class="mono" title={o && o.price == null ? 'No online orders on that side right now' : undefined}>
                {#if w.last_price != null}{w.last_price}p{:else}-{/if}
                <span class="muted"> · {ago(w.last_checked_at)}</span>
              </td>
              <td class={st.cls}>{st.label}</td>
              <td><button class="btn xs ghost" onclick={() => remove(w)} disabled={busyIds.has(w.id)}>Remove</button></td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}

  </section>
  <Toast {toasts} ondismiss={(id) => (toasts = toasts.filter((x) => x.id !== id))} />
</section>

<style>
  .row { display: flex; align-items: center; justify-content: space-between; gap: var(--s3); flex-wrap: wrap; }
  .gap-sm { gap: var(--s2); }
  .lede { margin: var(--s2) 0 0; color: var(--muted); }
  .add { display: flex; flex-wrap: wrap; gap: var(--s3); align-items: end; padding: var(--s4) var(--inset); }
  .add > .btn { min-height: var(--ctl-lg); }
  .pick-input { display: flex; gap: var(--s1); min-width: 0; }
  .watch-create { overflow: visible; }
  .watchlist { gap: var(--s4); }
  .threshold input { width: 7rem; }
  .pick { position: relative; flex: 1 1 16rem; min-width: 0; }
  .pick input { flex: 1; min-width: 0; }
  .suggest {
    position: absolute; top: 100%; left: 0; right: 0; z-index: var(--layer-menu);
    margin: 2px 0 0; padding: 4px; list-style: none;
    background: var(--panel); border: 1px solid var(--border); border-radius: var(--radius-panel);
    box-shadow: var(--shadow-pop);
  }
  .suggest li button {
    width: 100%; display: flex; justify-content: space-between; gap: var(--s3);
    background: none; border: 0; padding: 6px 8px; color: var(--fg); cursor: pointer; text-align: left; font: inherit; border-radius: var(--radius-ctl);
  }
  .suggest li button:hover { background: var(--panel-2); }
  table { min-width: 40rem; }
  .mono { font-family: var(--font-mono); font-variant-numeric: tabular-nums; white-space: nowrap; }
  .good { color: var(--good); font-weight: 600; }
  .bad { color: var(--bad); }
  .muted { color: var(--muted); }
  .empty { padding: 10px 0; }
  .scroll { overflow: auto; }
  @media (max-width: 35rem) {
    .pick { flex-basis: 100%; min-width: 0; }
    .add > label { min-width: 0; }
    .add select { max-width: 100%; }
  }
  table { min-width: 52rem; }
  td:last-child .btn { white-space: nowrap; }
  .ui-notice { margin: var(--s4) var(--inset); }
</style>
