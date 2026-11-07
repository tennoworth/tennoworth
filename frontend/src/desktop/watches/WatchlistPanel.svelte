<script lang="ts">
  import { fitTable } from '../../ui/table-fit';
  import { searchShortcut } from '../../ui/keyboard.svelte';
  import { busy, focusLater } from '../../ui/keyboard-actions';
  import { useDesktopServices } from '../shell/desktop-context';
  const { desktopAddWatch, desktopCheckWatchesNow, desktopDeleteWatch, desktopListWatches, listenForTauriEvent } = useDesktopServices();
  import { onMount, onDestroy } from 'svelte';

import { WATCH_FIRED_EVENT } from '../../contracts/events';
import { MAX_WATCHES, type NewWatch, type Watch, type WatchOutcome } from '../../contracts/desktop';
  
  import { buildBrowseIndex, searchItems, type BrowseRow } from '../../domain/market-browse';
  import { humanError } from '../../contracts/errors';
  import { plat } from '../../ui/format';
  import { t } from '../../i18n/i18n.svelte';
  import type { Market } from '../../contracts/data';
  import Toast from '../../ui/Toast.svelte';
  import Select from '../../ui/Select.svelte';
  import { createToastQueue } from '../../ui/toast-queue.svelte';

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
  let loadGeneration = 0;
  let disposed = false;

  // ---- add form ----
  let query = $state('');
  let picked = $state<BrowseRow | null>(null);
  let side = $state<'sell' | 'buy'>('sell');
  let threshold = $state<number>(0);
  let adding = $state(false);

  let index = $derived(buildBrowseIndex(market));
  let results = $derived(picked ? [] : searchItems(market, index, query, 8));
  // The suggestions are a listbox driven from the field: arrows choose, Enter
  // picks, Escape closes the list and a second Escape clears the field.
  let active = $state(-1);
  let suggestOpen = $state(true);
  let listShown = $derived(!picked && suggestOpen && results.length > 0);
  let atLimit = $derived(watches.length >= MAX_WATCHES);
  let confirmRemove = $state<number | null>(null);
  function searchKey(event: KeyboardEvent): void {
    if (event.isComposing) return;
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      if (!results.length || picked) return;
      event.preventDefault();
      if (!listShown) { suggestOpen = true; active = event.key === 'ArrowDown' ? 0 : results.length - 1; return; }
      active = event.key === 'ArrowDown' ? (active + 1) % results.length : (active - 1 + results.length) % results.length;
    } else if (event.key === 'Enter' && listShown) {
      event.preventDefault();
      pick(results[active >= 0 ? active : 0]);
    } else if (event.key === 'Escape' && listShown) {
      event.preventDefault(); event.stopPropagation();
      suggestOpen = false; active = -1;
    }
  }

  const toastQueue = createToastQueue(4500);
  const pushToast = toastQueue.push;
  onDestroy(() => {
    disposed = true;
    toastQueue.dispose();
  });

  async function load(): Promise<void> {
    const generation = ++loadGeneration;
    try {
      const next = await desktopListWatches();
      if (disposed || generation !== loadGeneration) return;
      watches = next;
      loadError = null;
    } catch (e) {
      if (disposed || generation !== loadGeneration) return;
      loadError = humanError(e);
    }
  }

  onMount(() => {
    void load();
    const stop = listenForTauriEvent<WatchOutcome>(WATCH_FIRED_EVENT, (o) => {
      const next = new Map(lastOutcomes);
      next.set(o.id, o);
      lastOutcomes = next;
      pushToast(describe(o));
      void load();
    });
    return () => { disposed = true; stop(); };
  });

  function pick(r: BrowseRow): void {
    picked = r;
    query = r.name;
    active = -1;
    focusLater(() => document.querySelector<HTMLInputElement>('.watchlist .threshold input'));
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
    loadGeneration++;
    try {
      const w: NewWatch = { slug: picked.slug, name: picked.name, side, threshold, rank: 0, subtype: null };
      const next = await desktopAddWatch(w);
      if (disposed) return;
      loadGeneration++;
      watches = next;
      pushToast(t('watches.watching', { item: picked.name, sign: side === 'sell' ? '≤' : '≥', price: threshold }));
      clearPick();
      focusLater(() => document.getElementById('watch-item'));
    } catch (e) {
      pushToast(humanError(e), 'error');
    } finally {
      adding = false;
    }
  }

  async function remove(w: Watch, row: number): Promise<void> {
    confirmRemove = null;
    busyIds = new Set([...busyIds, w.id]);
    loadGeneration++;
    try {
      const next = await desktopDeleteWatch(w.id);
      if (disposed) return;
      loadGeneration++;
      watches = next;
      pushToast(t('watches.stopped', { item: w.name }));
      // The row is gone: the next watch's Remove, or the item field, takes focus.
      focusLater(() => {
        const left = [...document.querySelectorAll<HTMLElement>('.watchlist .remove-watch')];
        return left[Math.min(row, left.length - 1)] ?? document.getElementById('watch-item');
      });
    } catch (e) {
      pushToast(humanError(e), 'error');
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
      pushToast(hits ? t('watches.satisfied', { count: hits }) : t('watches.noneSatisfied'));
      await load();
    } catch (e) {
      pushToast(humanError(e), 'error');
    } finally {
      checking = false;
    }
  }

  function describe(o: WatchOutcome): string {
    if (o.price == null) return t('watches.noData', { item: o.name });
    return t(o.side === 'sell' ? 'watches.listedAt' : 'watches.bidAt', { item: o.name, price: o.price, threshold: o.threshold });
  }

  function ago(unixSecs: number | null): string {
    if (unixSecs == null) return t('watches.notChecked');
    const s = Math.max(0, Math.floor(Date.now() / 1000) - unixSecs);
    if (s < 60) return t('format.justNow');
    if (s < 3600) return t('format.minutesAgo', { n: Math.floor(s / 60) });
    if (s < 86400) return t('format.hoursAgo', { n: Math.floor(s / 3600) });
    return t('format.daysAgo', { n: Math.floor(s / 86400) });
  }

  function status(w: Watch): { label: string; cls: string } {
    if (w.last_price == null) return { label: '-', cls: 'muted' };
    const hit = w.side === 'sell' ? w.last_price <= w.threshold : w.last_price >= w.threshold;
    return hit ? { label: t('watches.status.satisfied'), cls: 'good' } : { label: t('watches.status.waiting'), cls: 'muted' };
  }
</script>

<section class="ui-stack watchlist" data-testid="watchlist">
  <header class="view-header">
    <h2>{t('watches.title')}</h2><p class="lede">{t('watches.lede')}</p>
    <div class="vh-end">
      <button class="btn ghost" onclick={checkNow} disabled={watches.length === 0} use:busy={checking}
        title={t('watches.checkTitle')}>
        {t(checking ? 'watches.checking' : 'watches.checkNow')}
      </button>
    </div>
  </header>

  <section class="wrap tw watch-create" aria-labelledby="watch-create-title">
    <div class="rail"><h3 id="watch-create-title">{t('watches.add')}</h3></div>
  <form class="add" onsubmit={(event) => { event.preventDefault(); if (!atLimit) void add(); }}>
    <div class="pick ui-field">
      <label for="watch-item">{t('watches.item')}</label>
      <div class="pick-input">
      <input
        id="watch-item" use:searchShortcut
        class="ui-input"
        type="text"
        placeholder={t('watches.itemPlaceholder')}
        role="combobox"
        aria-autocomplete="list"
        aria-expanded={listShown}
        aria-controls="watch-suggest"
        aria-activedescendant={listShown && active >= 0 ? `watch-option-${active}` : undefined}
        bind:value={query}
        oninput={() => { if (picked && query !== picked.name) picked = null; suggestOpen = true; active = -1; }}
        onkeydown={searchKey}
        onblur={() => { suggestOpen = false; active = -1; }}
        onfocus={() => (suggestOpen = true)}
      />
      {#if picked}
        <button type="button" class="btn xs ghost" onclick={() => { clearPick(); focusLater(() => document.getElementById('watch-item')); }} aria-label={t('watches.clearItem')}>×</button>
      {/if}
      </div>
      <ul class="suggest" id="watch-suggest" role="listbox" aria-label={t('watches.matching')} hidden={!listShown}>
        {#if listShown}
          {#each results as r, i (r.slug)}
            <!-- svelte-ignore a11y_click_events_have_key_events (The field above drives the list from the keyboard.) -->
            <li id="watch-option-{i}" role="option" aria-selected={i === active} class:active={i === active}
              onpointerdown={(event) => event.preventDefault()} onclick={() => pick(r)}>
              <span class="name">{r.name}</span>
              <span class="muted">{plat(r.avg)}p · {r.vol}/48h</span>
            </li>
          {/each}
        {/if}
      </ul>
    </div>
    <div class="ui-field">
      <span>{t('watches.notifyWhen')}</span>
      <Select label={t('watches.notifyWhen')} value={side} options={[{ value: 'sell', label: t('watches.askBelow') }, { value: 'buy', label: t('watches.bidAbove') }]} onchange={(next) => { side = next; onSideChange(); }} />
    </div>
    <label class="ui-field threshold">
      <span>{t('watches.target')}</span>
      <input class="ui-input" type="number" min="1" bind:value={threshold} disabled={!picked} />
    </label>
    <button type="submit" class="btn primary" disabled={!picked || threshold < 1 || atLimit} use:busy={adding}
      aria-describedby={atLimit ? 'watch-limit' : undefined}>{t('watches.addButton')}</button>
  </form>
  {#if atLimit}<p class="ui-notice limit" data-tone="warn" id="watch-limit">{t('watches.limit', { count: MAX_WATCHES })}</p>{/if}

    <div class="foot">{t('watches.foot')}</div>
  </section>
  <section class="wrap tw" aria-labelledby="watch-list-title">
    <div class="rail"><h3 id="watch-list-title">{t('watches.yours')}</h3><span class="exp">{t('watches.count', { count: watches.length, max: MAX_WATCHES })}</span></div>
  {#if loadError}
    <div class="ui-notice" data-tone="bad" role="alert">{t('watches.loadFailed', { error: loadError })}</div>
  {:else if watches.length === 0}
    <div class="ui-notice">{t('watches.none')}</div>
  {:else}
    <div class="scroll" use:fitTable>
      <table class="tw fixed">
        <colgroup><col /><col style="width:10rem" /><col style="width:11rem" /><col style="width:8rem" /><col style="width:7rem" /></colgroup>
        <thead><tr><th class="l">{t('watches.col.item')}</th><th>{t('watches.col.condition')}</th><th>{t('watches.col.lastSeen')}</th><th>{t('watches.col.status')}</th><th>{t('watches.col.action')}</th></tr></thead>
        <tbody>
          {#each watches as w, row (w.id)}
            {@const st = status(w)}
            {@const o = lastOutcomes.get(w.id)}
            <tr>
              <td class="l">{w.name}{#if w.subtype}<span class="muted"> · {w.subtype}</span>{/if}</td>
              <td class="mono">{t(w.side === 'sell' ? 'watches.askAtMost' : 'watches.bidAtLeast')} {w.threshold}p</td>
              <td class="mono" title={o && o.price == null ? t('watches.noOrders') : undefined}>
                {#if w.last_price != null}{w.last_price}p{:else}-{/if}
                <span class="muted"> · {ago(w.last_checked_at)}</span>
              </td>
              <td class={st.cls}>{st.label}</td>
              <td>
                {#if confirmRemove === w.id}
                  <!-- svelte-ignore a11y_no_static_element_interactions (Escape backs out of the confirmation.) -->
                  <span class="confirm-pair" onkeydown={(event) => { if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); confirmRemove = null; focusLater(() => document.querySelector<HTMLElement>(`.remove-watch[data-watch="${w.id}"]`)); } }}>
                    <button class="btn xs bad" use:busy={busyIds.has(w.id)} onclick={() => remove(w, row)} aria-label={t('watches.confirmRemove', { item: w.name })}>{t('watches.remove')}</button>
                    <button class="btn xs ghost" onclick={() => { confirmRemove = null; focusLater(() => document.querySelector<HTMLElement>(`.remove-watch[data-watch="${w.id}"]`)); }} aria-label={t('watches.keepWatching', { item: w.name })}>{t('watches.keep')}</button>
                  </span>
                {:else}
                  <button class="btn xs ghost remove-watch" data-watch={w.id} onclick={() => { confirmRemove = w.id; focusLater(() => document.querySelector<HTMLElement>('.watchlist .confirm-pair .bad')); }} aria-label={t('watches.removeItem', { item: w.subtype ? `${w.name} · ${w.subtype}` : w.name })}>{t('watches.removeAsk')}</button>
                {/if}
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}

  </section>
  <Toast toasts={toastQueue.toasts} ondismiss={toastQueue.dismiss} />
</section>

<style>
  .row { display: flex; align-items: center; justify-content: space-between; gap: var(--s3); flex-wrap: wrap; }
  .gap-sm { gap: var(--s2); }
  .lede { margin: var(--s2) 0 0; color: var(--muted); }
  .add { display: flex; flex-wrap: wrap; gap: var(--s3); align-items: end; padding: var(--s4) var(--inset); }
  .add > .btn { min-height: var(--ctl-lg); }
  .pick-input { display: flex; gap: var(--s1); min-width: 0; }
  .watch-create { overflow: visible; }
  .watchlist { gap: var(--stack); }
  .threshold input { width: 7rem; }
  .pick { position: relative; flex: 0 1 28rem; min-width: 0; }
  .pick input { flex: 1; min-width: 0; }
  .suggest {
    position: absolute; top: 100%; left: 0; right: 0; z-index: var(--layer-menu);
    margin: 2px 0 0; padding: 4px; list-style: none;
    background: var(--panel); border: 1px solid var(--border); border-radius: var(--radius-panel);
    box-shadow: var(--shadow-pop);
  }
  .suggest li {
    display: flex; justify-content: space-between; gap: var(--s3);
    padding: 6px 8px; color: var(--fg); cursor: pointer; border-radius: var(--radius-ctl);
  }
  .suggest li:hover { background: var(--panel-2); }
  /* The keyboard's current option: focus stays in the field, so the option
     carries the focus ring itself. */
  .suggest li.active { background: var(--panel-2); outline: 2px solid var(--accent); outline-offset: -2px; }
  .limit { margin: 0 var(--inset) var(--s3); }
  table { min-width: 40rem; }
  .mono { font-family: var(--font-mono); font-variant-numeric: tabular-nums; white-space: nowrap; }
  .good { color: var(--good); font-weight: 600; }
  .bad { color: var(--bad); }
  .muted { color: var(--muted); }
  .empty { padding: 10px 0; }
  .scroll { overflow: auto; }
  @media (max-width: 35rem) {
    .pick { flex-basis: 100%; min-width: 0; }
    .add > .ui-field { min-width: 0; }
  }
  table { min-width: 52rem; }
  td:last-child .btn { white-space: nowrap; }
  .ui-notice { margin: var(--s4) var(--inset); }
</style>
