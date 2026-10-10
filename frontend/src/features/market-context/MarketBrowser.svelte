<script lang="ts">
  import { buildMetaDrift } from '../../domain/meta-drift';
  import { baroLocation, humanWindow, plat, wfmItemUrl } from '../../ui/format';
  import { onMount, onDestroy, type Snippet } from 'svelte';
  import type { Market } from '../../contracts/data';
  import {
    buildBrowseIndex,
    searchItems,
    topMovers,
    vaultedTop,
    dispositionChanges,
    handoffSample,
    type BrowseRow,
    type HandoffRow,
  } from '../../domain/market-browse';
  import Sparkline from '../../ui/Sparkline.svelte';
  import { hasSparkline } from '../../ui/sparkline';
  import { weekly, yearStats, type History } from '../../domain/history';
  import MetaDriftPanel from './MetaDriftPanel.svelte';
  import { shimmer, type ShimmerMode } from '../../ui/shimmer';

  // Powered by the already-loaded market.json - the only fetch this component
  // can trigger is the optional year-long history, and only when the user
  // flips the "1 year" toggle (App passes the transport's loader so the
  // desktop keeps its egress in Rust).
  //
  // Flow v2 landing: every list is a table on the workspace's row anatomy
  // (28px head · 32px rows · mono numbers · same column names), so a visitor
  // reads the same columns here that the app ranks their inventory by. The
  // search results carry three ghost columns (Own · Priority · Stack value) that
  // only the desktop scan can fill; `handoff` renders the same rows completed.
  let {
    market,
    staleness = null,
    freshness = 'unknown',
    loadHistory = null,
    handoff = undefined,
  }: {
    market: Market;
    staleness?: string | null;
    freshness?: 'fresh' | 'aging' | 'stale' | 'unknown';
    loadHistory?: (() => Promise<History | null>) | null;
    /** Hosted only: the hand-off panel (same rows, completed + install). */
    handoff?: Snippet<[HandoffRow[]]>;
  } = $props();

  let query = $state('');
  let searchInput: HTMLInputElement | undefined = $state();
  let searchFocused = $state(false);
  // Quietly shimmers at rest; in full while focused and waiting for a query, and
  // quiet again once results show so the eye moves to the table.
  let searchShimmer = $derived<ShimmerMode>(searchFocused && !query.trim() ? 'live' : 'idle');

  // '/' focuses the search from anywhere on the landing (unless already typing).
  $effect(() => {
    const handler = (e: KeyboardEvent): void => {
      if (e.key !== '/' || e.ctrlKey || e.metaKey || e.altKey) return;
      const t = e.target as HTMLElement | null;
      if (t && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA' || t.isContentEditable)) return;
      e.preventDefault();
      searchInput?.focus();
    };
    document.addEventListener('keydown', handler);
    return () => document.removeEventListener('keydown', handler);
  });

  // ---- 1-year view (history.json, on demand) ----
  let showYear = $state(false);
  let history = $state<History | null>(null);
  type HistState = 'idle' | 'loading' | 'ready' | 'unavailable';
  let histState = $state<HistState>('idle');
  async function toggleYear(): Promise<void> {
    showYear = !showYear;
    if (showYear && histState === 'idle' && loadHistory) {
      histState = 'loading';
      history = await loadHistory();
      histState = history ? 'ready' : 'unavailable';
    }
  }
  function yearFor(slug: string) {
    const s = history?.items[slug];
    if (!s) return null;
    const stats = yearStats(s);
    if (!stats) return null;
    return { stats, spark: weekly(s, 52) };
  }
  let yearMode = $derived(showYear && histState === 'ready');

  // Index + the standing reports are pure derivations of the snapshot.
  let index = $derived(buildBrowseIndex(market));
  let results = $derived(searchItems(market, index, query, 12));
  let movers = $derived(topMovers(market, index, { minVol: 20, minPrice: 10, limit: 8 }));
  let vaulted = $derived(vaultedTop(market, index, 8));
  let dispoChanges = $derived(dispositionChanges(market, 12));
  const CONTEXT_PREVIEW = 10;
  const BARO_PREVIEW = 5;
  let dispoAll = $state(false);
  let baroStockAll = $state(false);
  let hasMeta = $derived(!!buildMetaDrift(market));
  let contextTab = $state<'meta' | 'dispo'>('meta');
  let contextTabs = $derived([
    ...(hasMeta ? [{ id: 'meta' as const, label: 'Meta drift', count: null }] : []),
    ...(dispoChanges.length ? [{ id: 'dispo' as const, label: 'Riven dispositions', count: dispoChanges.length }] : []),
  ]);
  let shownTab = $derived(contextTabs.some((t) => t.id === contextTab) ? contextTab : contextTabs[0]?.id);
  // Arrow keys, Home and End move between the tabs, as in the feature rail.
  function onContextKey(event: KeyboardEvent): void {
    const ids = contextTabs.map((t) => t.id);
    const at = ids.indexOf(shownTab ?? ids[0]);
    const next = event.key === 'ArrowRight' ? (at + 1) % ids.length : event.key === 'ArrowLeft' ? (at - 1 + ids.length) % ids.length : event.key === 'Home' ? 0 : event.key === 'End' ? ids.length - 1 : -1;
    if (next < 0) return;
    event.preventDefault();
    contextTab = ids[next];
    document.getElementById(`ctx-tab-${ids[next]}`)?.focus();
  }
  let sample = $derived(handoff ? handoffSample(market, index) : []);
  function dispoDelta(from: number, to: number): string {
    const d = to - from;
    return `${d > 0 ? '+' : ''}${d.toFixed(2)}`;
  }
  function seenDate(iso: string): string {
    const t = Date.parse(iso);
    return Number.isFinite(t) ? new Date(t).toLocaleDateString(undefined, { month: 'short', day: 'numeric' }) : '';
  }

  // Baro schedule + (since 2026-08-10) the last captured stock. market.json
  // carries activation/expiry/location and, when the scrape saw a visit, his
  // inventory - joined to prices through the catalog so the card can say what
  // each ducat item averages on WFM right now.
  let baro = $derived.by(() => {
    const b = market?.baro;
    if (!b) return null;
    return { ...b, location: baroLocation(b.location) };
  });
  let baroStock = $derived.by(() => {
    const inv = market?.baro?.inventory;
    if (!Array.isArray(inv) || inv.length === 0) return [];
    const catalog = market?.catalog ?? {};
    const items = market?.items ?? {};
    return inv
      .filter((s) => s && typeof s.item === 'string')
      .map((s) => {
        const slug = catalog[s.item.toLowerCase()];
        const e = slug ? items[slug] : undefined;
        return { name: s.item, ducats: s.ducats ?? null, avg: e?.avg ?? null, slug: slug ?? null };
      })
      .sort((a, b) => (b.ducats ?? 0) - (a.ducats ?? 0))
      .slice(0, 8);
  });
  let baroStockIsPast = $derived.by(() => {
    const b = market?.baro;
    return Boolean(b?.inventory_for && b.activation && b.inventory_for !== b.activation);
  });

  // A minute-resolution clock so the countdown ticks without a reload. Written
  // only by the interval (never read+written inside an $effect).
  let now = $state(Date.now());
  let timer: ReturnType<typeof setInterval> | undefined;
  onMount(() => {
    timer = setInterval(() => { now = Date.now(); }, 60000);
  });
  onDestroy(() => { if (timer) clearInterval(timer); });

  let baroState = $derived.by(() => {
    if (!baro) return null;
    const arr = Date.parse(baro.activation);
    const exp = Date.parse(baro.expiry);
    if (Number.isFinite(exp) && now < exp && Number.isFinite(arr) && now >= arr) {
      return { phase: 'here' as const, label: 'leaves in', windowMs: exp - now };
    }
    if (Number.isFinite(arr) && now < arr) {
      return { phase: 'incoming' as const, label: 'arrives in', windowMs: arr - now };
    }
    return { phase: 'unknown' as const, label: 'next visit', windowMs: null };
  });

  function ratioText(r: number): string {
    return Number.isFinite(r) ? r.toFixed(2) : '-';
  }
</script>

<!-- Item cell: name link + vault tag. Same in every table. -->
{#snippet itemCell(r: BrowseRow)}
  <a href={wfmItemUrl(r.slug)} target="_blank" rel="noopener noreferrer" title={r.name}>{r.name}</a>
  {#if r.vault === 'vaulted'}
    <span class="tag vaulted" title="Vaulted - no longer dropping, supply is capped">vaulted</span>
  {:else if r.vault === 'vaulting-soon'}
    <span class="tag soon" title="Vaulting soon - supply about to be capped">soon</span>
  {/if}
{/snippet}

<!-- Δ vs the 90-day median (or vs a year ago in the 1-year view). -->
{#snippet deltaCell(r: BrowseRow)}
  {#if yearMode}
    {@const y = yearFor(r.slug)}
    {#if y && y.stats.deltaPct != null && Math.abs(y.stats.deltaPct) >= 1}
      {#if y.stats.deltaPct > 0}
        <span class="up" title="Latest daily median {y.stats.deltaPct.toFixed(0)}% above where it was a year ago ({y.stats.baseline}p → {y.stats.latest}p; year low {y.stats.low}p, high {y.stats.high}p)">▲{y.stats.deltaPct.toFixed(0)}% 1y</span>
      {:else}
        <span class="down" title="Latest daily median {Math.abs(y.stats.deltaPct).toFixed(0)}% below where it was a year ago ({y.stats.baseline}p → {y.stats.latest}p; year low {y.stats.low}p, high {y.stats.high}p)">▼{Math.abs(y.stats.deltaPct).toFixed(0)}% 1y</span>
      {/if}
    {:else}
      <span class="flat">·</span>
    {/if}
  {:else if r.deltaPct != null && Math.abs(r.deltaPct) >= 1}
    {#if r.deltaPct > 0}
      <span class="up" title="Latest median {r.deltaPct.toFixed(0)}% above the 90-day median">▲{r.deltaPct.toFixed(0)}%</span>
    {:else}
      <span class="down" title="Latest median {Math.abs(r.deltaPct).toFixed(0)}% below the 90-day median">▼{Math.abs(r.deltaPct).toFixed(0)}%</span>
    {/if}
  {:else}
    <span class="flat" title="Within ±1% of the 90-day median">·</span>
  {/if}
{/snippet}

<!-- 7-day (or 52-week) sparkline. -->
{#snippet trendCell(r: BrowseRow, w: number, h: number)}
  {#if yearMode}
    {@const y = yearFor(r.slug)}
    {#if y}
      {#if hasSparkline(y.spark)}
        <Sparkline class="spark year" series={y.spark} width={w} height={h}
          title="Weekly medians over the last year: low {y.stats.low}p, high {y.stats.high}p, {y.stats.tradedDays} traded days" />
      {/if}
    {:else}
      <span class="thin-hist" title="Fewer than 20 traded days in the last year">thin history</span>
    {/if}
  {:else if hasSparkline(r.medians_7d)}
    <Sparkline series={r.medians_7d} width={w} height={h} baseline={r.median90} trend={r.deltaPct}
      title="7-day medians: {r.medians_7d.join(', ')}{r.median90 != null ? ` · 90-day median ${r.median90}` : ''}" />
  {:else}
    <span class="faint">-</span>
  {/if}
{/snippet}

<!-- A price cell's value; missing stays "-" without a unit. -->
{#snippet price(v: number | null | undefined)}{plat(v)}{#if plat(v) !== '-'}<span class="unit">p</span>{/if}{/snippet}

<!-- Mini table: Item · Δ 90d · Trend · Avg · Vol 48h (+ Ducats) - movers and vaulted. -->
{#snippet miniTable(rows: BrowseRow[], sortedKey: 'delta' | 'avg', ducats: boolean, emptyText: string)}
  {#if rows.length}
    <div class="scroll">
    <table class="tw fixed mini-table" class:with-ducats={ducats}>
      <colgroup>
        <col />
        <col style="width:3.75rem" />
        <col style="width:4.5rem" />
        <col style="width:3.5rem" />
        <col style="width:4rem" />
        {#if ducats}<col style="width:4rem" />{/if}
      </colgroup>
      <thead><tr>
        <th class="l">Item</th>
        <th class:sorted={sortedKey === 'delta'} title="Latest daily median vs the 90-day median">{yearMode ? 'Δ 1y' : 'Δ 90d'}</th>
        <th title="{yearMode ? 'Weekly medians, last year' : 'Daily medians, last 7 days'}">Trend</th>
        <th class:sorted={sortedKey === 'avg'} title="Average of recent WFM sales">Avg</th>
        <th title="Trades completed in the last 48 hours">Vol 48h</th>
        {#if ducats}<th title="Ducat value at Baro Ki’Teer">Ducats</th>{/if}
      </tr></thead>
      <tbody>
        {#each rows as r (r.slug)}
          <tr>
            <td class="l">{@render itemCell(r)}</td>
            <td>{@render deltaCell(r)}</td>
            <td>{@render trendCell(r, 56, 16)}</td>
            <td class="price">{@render price(r.avg)}</td>
            <td>{r.vol.toLocaleString()}</td>
            {#if ducats}<td>{#if r.ducats != null}<span class="ducat">{r.ducats}</span>{:else}<span class="faint">-</span>{/if}</td>{/if}
          </tr>
        {/each}
      </tbody>
    </table>
    </div>
  {:else}
    <div class="body"><p>{emptyText}</p></div>
  {/if}
{/snippet}

<section class="browser" data-testid="market-browser">

  <!-- 1. LOOK SOMETHING UP - the first control on the page. Idle = the bar
       alone; typing renders the results table under it. -->
  <section class="wrap tw lookup" aria-label="Item lookup">
    <div class="bar">
      <span class="shimmer-field grow" use:shimmer={{ mode: searchShimmer, sparks: true }}>
        <input
          class="input"
          type="text"
          placeholder="Search any item - try “primed”, “mag”, “ash prime set”…"
          bind:value={query}
          bind:this={searchInput}
          aria-label="Search items"
          onfocus={() => (searchFocused = true)}
          onblur={() => (searchFocused = false)}
        />
      </span>
      <span class="exp">any tradeable item · price, 48h volume, 7-day trend · <kbd>/</kbd> to focus</span>
      {#if loadHistory}
        <button class="btn xs ghost year-toggle" class:on={showYear} onclick={toggleYear} aria-pressed={showYear}
          title="Show each item's last year of daily prices (relics.run archive) instead of the last 7 days">
          {histState === 'loading' ? 'Loading 1 year…' : '1 year'}
        </button>
        {#if showYear && histState === 'unavailable'}<span class="exp note">· history unavailable right now</span>{/if}
        {#if showYear && histState === 'ready' && history?.through}<span class="exp note">· through {history.through}</span>{/if}
      {/if}
    </div>
    {#if freshness === 'stale'}
      <div class="line stale-note" data-tone="warn">⚠ This snapshot was updated {staleness ?? 'at an unknown time'} - prices below may lag the live book.</div>
    {/if}
    {#if query.trim()}
      {#if results.length}
        <div class="scroll">
        <table class="tw fixed results">
          <colgroup>
            <col />
            <col style="width:3.75rem" />
            <col style="width:4.75rem" />
            <col style="width:3.5rem" />
            <col style="width:4.25rem" />
            <col style="width:4.25rem" />
            <col style="width:4rem" />
            <col style="width:4rem" />
            <col style="width:4rem" />
            <col style="width:3.5rem" />
            <col style="width:5.5rem" />
            <col style="width:7rem" />
          </colgroup>
          <thead><tr>
            <th class="l">Item · {results.length} {results.length === 1 ? 'match' : 'matches'}</th>
            <th title="Latest daily median vs the 90-day median">{yearMode ? 'Δ 1y' : 'Δ 90d'}</th>
            <th title="{yearMode ? 'Weekly medians, last year' : 'Daily medians, last 7 days'}">Trend</th>
            <th title="Average of recent WFM sales - list below it to sell faster">Avg</th>
            <th title="Lowest current online sell listing">Low ask</th>
            <th title="Highest current online buy offer">Top buy</th>
            <th title="Trades completed in the last 48 hours">Vol 48h</th>
            <th title="Live buyers ÷ live sellers - > 1 means buyers outnumber sellers">Demand</th>
            <th title="Ducat value at Baro Ki’Teer">Ducats</th>
            <th class="ghost g1" title="How many you own - filled by the desktop scan">Own</th>
            <th class="ghost" title="Priority from price, likely sell-through, and bounded DE usage - filled by the desktop scan">Priority</th>
            <th class="ghost" title="Sellable copies × the 48 h average trade price - filled by the desktop scan">Stack value</th>
          </tr></thead>
          <tbody>
            {#each results as r (r.slug)}
              <tr>
                <td class="l">{@render itemCell(r)}</td>
                <td>{@render deltaCell(r)}</td>
                <td>{@render trendCell(r, 60, 18)}</td>
                <td class="price">{@render price(r.avg)}</td>
                <td>{plat(r.lowSell)}</td>
                <td>{plat(r.topBuy)}</td>
                <td>{r.vol.toLocaleString()}</td>
                <td>{ratioText(r.ratio)}</td>
                <td>{#if r.ducats != null}<span class="ducat">{r.ducats}</span>{:else}<span class="faint">-</span>{/if}</td>
                <td class="ghost g1">·</td>
                <td class="ghost">·</td>
                <td class="ghost">·</td>
              </tr>
            {/each}
          </tbody>
        </table>
        </div>
        <div class="line">
          {#if handoff}
            <span class="exp">Own · Priority · Stack value fill in when the <a href="#desktop">desktop app</a> scans your inventory - free, Windows + Linux, no login.</span>
            <span class="grow"></span>
            <a href="#desktop">↓ see the completed row</a>
          {:else}
            <span class="exp">Own · Priority · Stack value fill in once you scan your inventory (Refresh ▾ → Scan game).</span>
          {/if}
        </div>
      {:else}
        <div class="line"><span class="exp">No priceable items match “{query.trim()}”.</span></div>
      {/if}
    {/if}
  </section>

  <!-- 2. MOVERS - two mini-tables, same column heads as the workspace -->
  <section class="two">
    <div class="wrap tw movers">
      <!-- Twin rails: each list is titled, so neither depends on the other's heading. -->
      <div class="rail">
        <h3 title="Compares the latest price to the 90-day median. Only items with 20+ sales in 48 h qualify, so one fluke sale can't move the list.">▲ Rising</h3>
        <span class="exp">vs 90-day median · vol ≥ 20</span>
      </div>
      {@render miniTable(movers.risers, 'delta', false, 'No risers.')}
    </div>
    <div class="wrap tw movers">
      <div class="rail">
        <h3 title="Compares the latest price to the 90-day median. Only items with 20+ sales in 48 h qualify, so one fluke sale can't move the list.">▼ Falling</h3>
        <span class="exp">vs 90-day median · vol ≥ 20</span>
      </div>
      {@render miniTable(movers.fallers, 'delta', false, 'No fallers.')}
    </div>
  </section>

  <!-- 3. VAULTED (2/3) + BARO (1/3) -->
  <section class="two-one">
    <div class="wrap tw vaulted">
      <div class="rail">
        <h3>Vaulted &amp; valuable</h3>
        <span class="exp">no longer drop, so supply is capped</span>
      </div>
      {@render miniTable(vaulted, 'avg', true, 'No vault data in this snapshot.')}
      <div class="line"><span class="exp">High-value vaulted items tend to hold or climb.</span></div>
    </div>
    {#if baro && baroState}
      <div class="wrap tw baro">
        <div class="rail">
          <span class="glyph" aria-hidden="true">⌬</span>
          <h3>Baro Ki'Teer</h3>
          <span class="exp">{baro.location}</span>
        </div>
        <div class="body">
          <div class="clock"><small>{baroState.label}</small>{humanWindow(baroState.windowMs)}</div>
          <p>
            {#if baroState.phase === 'here'}
              At {baro.location} now.
            {:else if baroState.phase === 'incoming'}
              Arrives at {baro.location}.
            {:else}
              Next visit at {baro.location}.
            {/if}
            Schedule only - bring your own ducats. His mods dip ~50% on arrival day; the money is in holding for the recovery.
          </p>
        </div>
        {#if baroStock.length}
          <div class="scroll">
          <table class="tw fixed baro-stock">
            <colgroup><col /><col style="width:4rem" /><col style="width:4.5rem" /></colgroup>
            <thead><tr>
              <th class="l">{baroStockIsPast ? "Last visit's stock" : 'Stock'}</th>
              <th title="Ducat price at Baro">Ducats</th>
              <th title="Average of recent WFM sales">Avg now</th>
            </tr></thead>
            <tbody>
              {#each baroStockAll ? baroStock : baroStock.slice(0, BARO_PREVIEW) as s (s.name)}
                <tr>
                  <td class="l">{#if s.slug}<a href={wfmItemUrl(s.slug)} target="_blank" rel="noopener noreferrer">{s.name}</a>{:else}{s.name}{/if}</td>
                  <td>{#if s.ducats != null}<span class="ducat">{s.ducats}</span>{:else}<span class="faint">-</span>{/if}</td>
                  <td class="fg">{s.avg != null ? plat(s.avg) : '-'}</td>
                </tr>
              {/each}
            </tbody>
          </table>
          </div>
        {/if}
        <div class="line">
          {#if baroStock.length > BARO_PREVIEW}<button type="button" class="btn xs" aria-expanded={baroStockAll} onclick={() => (baroStockAll = !baroStockAll)}>{baroStockAll ? `Show top ${BARO_PREVIEW}` : `All ${baroStock.length} items`}</button>{/if}
          {#if handoff}<a href="#desktop">Ducat math for what you own →</a>{/if}
        </div>
      </div>
    {/if}
  </section>

  <!-- 4. HAND-OFF: the same rows, completed by the desktop app (hosted only),
       straight after the lists a visitor came for -->
  {#if handoff}
    {@render handoff(sample)}
  {/if}

  <!-- 5. MARKET CONTEXT: the slower signals behind prices, as two tabs of one
       panel rather than two long stacked tables. Each shows its top ten. -->
  {#if hasMeta || dispoChanges.length}
    <section class="context" aria-label="Market context">
      <div class="ui-segmented context-tabs" role="tablist" aria-label="Market context" tabindex="-1" onkeydown={onContextKey}>
        {#each contextTabs as t (t.id)}
          <button type="button" role="tab" id="ctx-tab-{t.id}" aria-controls="ctx-panel-{t.id}" aria-selected={shownTab === t.id} tabindex={shownTab === t.id ? 0 : -1} onclick={() => (contextTab = t.id)}>{t.label}{#if t.count != null} <span class="n">{t.count}</span>{/if}</button>
        {/each}
      </div>
      {#if shownTab === 'meta'}
        <div role="tabpanel" id="ctx-panel-meta" aria-labelledby="ctx-tab-meta"><MetaDriftPanel {market} limit={CONTEXT_PREVIEW} embedded /></div>
      {:else if dispoChanges.length}
        <div role="tabpanel" id="ctx-panel-dispo" aria-labelledby="ctx-tab-dispo">
        <section class="wrap tw dispo" data-testid="dispo-changes">
          <div class="rail">
            <h3>Riven disposition changes</h3>
            <span class="exp">last 90 days · DE only raises dispositions now, so each change is a one-way price event for that weapon's rivens - WFM reprices within a day</span>
          </div>
      <div class="scroll">
      <table class="tw fixed dispo-table">
        <colgroup><col /><col style="width:8rem" /><col style="width:4rem" /><col style="width:5rem" /></colgroup>
        <thead><tr>
          <th class="l">Weapon</th>
          <th title="Disposition before → after">Disposition</th>
          <th>Δ</th>
          <th title="When our scrape first saw the new value">Seen</th>
        </tr></thead>
        <tbody>
          {#each dispoAll ? dispoChanges : dispoChanges.slice(0, CONTEXT_PREVIEW) as c (c.slug + c.seen_at)}
            <tr>
              <td class="l">{c.name}</td>
              <td class:up={c.to > c.from} class:down={c.to < c.from} title={`Disposition ${c.from.toFixed(2)} → ${c.to.toFixed(2)}`}>{c.from.toFixed(2)} → <strong>{c.to.toFixed(2)}</strong></td>
              <td class:up={c.to > c.from} class:down={c.to < c.from}>{dispoDelta(c.from, c.to)}</td>
              <td>{seenDate(c.seen_at)}</td>
            </tr>
          {/each}
        </tbody>
      </table>
      </div>
          {#if dispoChanges.length > CONTEXT_PREVIEW}
            <div class="line"><span class="exp">{dispoAll ? `All ${dispoChanges.length}` : `Top ${CONTEXT_PREVIEW} of ${dispoChanges.length}`}</span><span class="grow"></span><button type="button" class="btn xs" aria-expanded={dispoAll} onclick={() => (dispoAll = !dispoAll)}>{dispoAll ? `Show top ${CONTEXT_PREVIEW}` : `Show all ${dispoChanges.length}`}</button></div>
          {/if}
        </section>
        </div>
      {/if}
    </section>
  {/if}
</section>

<style>
  .browser { display: flex; flex-direction: column; gap: var(--stack); min-width: 0; }
  .two { display: grid; grid-template-columns: 1fr 1fr; gap: var(--stack); min-width: 0; }
  .two-one { display: grid; grid-template-columns: 2fr 1fr; gap: var(--stack); min-width: 0; }
  @media (max-width: 900px) {
    .two, .two-one { grid-template-columns: 1fr; }
  }

  .lookup .bar .exp kbd {
    font-family: var(--font-mono);
    font-size: var(--text-caption);
    border: 1px solid var(--border);
    border-radius: var(--radius-tag);
    padding: 0 4px;
    color: var(--fg);
  }
  .lookup .exp.note { flex-shrink: 0; }
  .year-toggle.on { color: var(--fg); border-color: var(--accent); }
  .stale-note { color: var(--warn); }
  .thin-hist { font-size: var(--text-caption); color: var(--muted); font-family: var(--font-body); }
  /* Below the results table's natural width the panel pans sideways rather
     than squeezing the Item column. */
  .results { min-width: 59.25rem; }
  .mini-table { min-width: 27rem; }
  .mini-table.with-ducats { min-width: 31rem; }
  .baro-stock { min-width: 27rem; }
  .dispo-table { min-width: 34rem; }
  .dispo-table strong { font-weight: 600; }
  .context { display: flex; flex-direction: column; gap: var(--s2); min-width: 0; }
  .context-tabs { align-self: flex-start; }
  .context-tabs .n { font-family: var(--font-mono); font-size: var(--text-caption); }
  .wrap.vaulted { display: flex; flex-direction: column; }
  .wrap.vaulted > .line { margin-top: auto; }
  .baro .line { display: flex; flex-wrap: wrap; align-items: center; gap: var(--s3); }

  @media (max-width: 35rem) {
    .lookup .bar .shimmer-field { flex-basis: 100%; }
    .lookup .bar .year-toggle { margin-left: auto; }
  }

  /* Baro card: rail glyph in ducat gold, mono clock. */
  .baro { display: flex; flex-direction: column; }
  .baro .glyph { color: var(--ducat); font-size: var(--text-body); }
  .baro .body { display: flex; flex-direction: column; gap: var(--s2); }
  .baro .clock {
    font-family: var(--font-mono);
    font-size: var(--text-metric);
    line-height: 1.5rem;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
    color: var(--fg);
  }
  .baro .clock small {
    font: 600 var(--text-caption)/1rem var(--font-ui);
    color: var(--muted);
    letter-spacing: 0.1em;
    text-transform: uppercase;
    margin-right: var(--s2);
  }
  .baro .line { margin-top: auto; }
  .baro .line a { color: var(--accent); }
  .lookup .line a { color: var(--accent); }
</style>
