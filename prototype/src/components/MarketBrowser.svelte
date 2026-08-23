<script lang="ts">
  import { baroLocation, humanWindow, plat, wfmItemUrl } from '../lib/format';
  import { onMount, onDestroy, type Snippet } from 'svelte';
  import type { Market } from '../lib/types';
  import {
    buildBrowseIndex,
    searchItems,
    topMovers,
    vaultedTop,
    dispositionChanges,
    handoffSample,
    type BrowseRow,
    type HandoffRow,
  } from '../lib/market-browse';
  import { sparklinePoints } from '../lib/sparkline';
  import { weekly, yearStats, type History } from '../lib/history';
  import MetaDriftPanel from './MetaDriftPanel.svelte';

  // Powered by the already-loaded market.json — the only fetch this component
  // can trigger is the optional year-long history, and only when the user
  // flips the "1 year" toggle (App passes the transport's loader so the
  // desktop keeps its egress in Rust).
  //
  // Flow v2 landing: every list is a table on the workspace's row anatomy
  // (28px head · 32px rows · mono numbers · same column names), so a visitor
  // reads the same columns here that the app ranks their inventory by. The
  // search results carry three ghost columns (Own · Score · Potential) that
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
  // inventory — joined to prices through the catalog so the card can say what
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
    return Number.isFinite(r) ? r.toFixed(2) : '—';
  }
</script>

<!-- Item cell: name link + vault tag. Same in every table. -->
{#snippet itemCell(r: BrowseRow)}
  <a href={wfmItemUrl(r.slug)} target="_blank" rel="noopener noreferrer" title={r.name}>{r.name}</a>
  {#if r.vault === 'vaulted'}
    <span class="tag vaulted" title="Vaulted — no longer dropping, supply is capped">vaulted</span>
  {:else if r.vault === 'vaulting-soon'}
    <span class="tag soon" title="Vaulting soon — supply about to be capped">soon</span>
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
      {#if sparklinePoints(y.spark, w, h)}
        <svg class="spark year" viewBox="0 0 {w} {h}" width={w} height={h} aria-hidden="true">
          <title>Weekly medians over the last year: low {y.stats.low}p, high {y.stats.high}p, {y.stats.tradedDays} traded days</title>
          <polyline points={sparklinePoints(y.spark, w, h)} fill="none" stroke="currentColor" stroke-width="1.25" />
        </svg>
      {/if}
    {:else}
      <span class="thin-hist" title="Fewer than 20 traded days in the last year">thin history</span>
    {/if}
  {:else if sparklinePoints(r.medians_7d, w, h)}
    <svg class="spark" viewBox="0 0 {w} {h}" width={w} height={h} aria-hidden="true">
      <title>7-day medians: {r.medians_7d?.join(', ')}</title>
      <polyline points={sparklinePoints(r.medians_7d, w, h)} fill="none" stroke="currentColor" stroke-width="1.25" />
    </svg>
  {:else}
    <span class="faint">—</span>
  {/if}
{/snippet}

<!-- Mini table: Item · Δ 90d · Trend · Avg · Vol 48h (+ Ducats) — movers and vaulted. -->
{#snippet miniTable(rows: BrowseRow[], sortedKey: 'delta' | 'avg', ducats: boolean, emptyText: string)}
  {#if rows.length}
    <table class="tw fixed">
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
            <td class="fg">{plat(r.avg)}</td>
            <td>{r.vol.toLocaleString()}</td>
            {#if ducats}<td>{#if r.ducats != null}<span class="ducat">{r.ducats}</span>{:else}<span class="faint">—</span>{/if}</td>{/if}
          </tr>
        {/each}
      </tbody>
    </table>
  {:else}
    <div class="body"><p>{emptyText}</p></div>
  {/if}
{/snippet}

<section class="browser" data-testid="market-browser">

  <!-- 1. LOOK SOMETHING UP — the first control on the page. Idle = the bar
       alone; typing renders the results table under it. -->
  <section class="wrap tw lookup" aria-label="Item lookup">
    <div class="bar">
      <input
        class="input grow"
        type="text"
        placeholder="Search any item — try “primed”, “mag”, “ash prime set”…"
        bind:value={query}
        bind:this={searchInput}
        aria-label="Search items"
      />
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
      <div class="line stale-note">⚠ This snapshot is {staleness} old — prices below may lag the live book.</div>
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
            <col style="width:4rem" />
            <col style="width:5.25rem" />
          </colgroup>
          <thead><tr>
            <th class="l">Item · {results.length} {results.length === 1 ? 'match' : 'matches'}</th>
            <th title="Latest daily median vs the 90-day median">{yearMode ? 'Δ 1y' : 'Δ 90d'}</th>
            <th title="{yearMode ? 'Weekly medians, last year' : 'Daily medians, last 7 days'}">Trend</th>
            <th title="Average of recent WFM sales — list below it to sell faster">Avg</th>
            <th title="Lowest current online sell listing">Low sell</th>
            <th title="Highest current online buy offer">Top buy</th>
            <th title="Trades completed in the last 48 hours">Vol 48h</th>
            <th title="Live buyers ÷ live sellers — > 1 means buyers outnumber sellers">Demand</th>
            <th title="Ducat value at Baro Ki’Teer">Ducats</th>
            <th class="ghost g1" title="How many you own — filled by the desktop scan">Own</th>
            <th class="ghost" title="Prioritization score from price, likely sell-through, and bounded DE usage — filled by the desktop scan">Score</th>
            <th class="ghost" title="Owned × Avg — filled by the desktop scan">Potential</th>
          </tr></thead>
          <tbody>
            {#each results as r (r.slug)}
              <tr>
                <td class="l">{@render itemCell(r)}</td>
                <td>{@render deltaCell(r)}</td>
                <td>{@render trendCell(r, 60, 18)}</td>
                <td class="fg">{plat(r.avg)}</td>
                <td>{plat(r.lowSell)}</td>
                <td>{plat(r.topBuy)}</td>
                <td>{r.vol.toLocaleString()}</td>
                <td>{ratioText(r.ratio)}</td>
                <td>{#if r.ducats != null}<span class="ducat">{r.ducats}</span>{:else}<span class="faint">—</span>{/if}</td>
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
            <span class="exp">Own · Score · Potential fill in when the <a href="#desktop">desktop app</a> scans your inventory — free, Windows + Linux, no login.</span>
            <span class="grow"></span>
            <a href="#desktop">↓ see the completed row</a>
          {:else}
            <span class="exp">Own · Score · Potential fill in once you scan your inventory (Refresh ▾ → Scan game).</span>
          {/if}
        </div>
      {:else}
        <div class="line"><span class="exp">No priceable items match “{query.trim()}”.</span></div>
      {/if}
    {/if}
  </section>

  <!-- 2. MOVERS — two mini-tables, same column heads as the workspace -->
  <section class="two">
    <div class="wrap tw movers">
      <div class="rail">
        <h3 title="Compares the latest price to the 90-day median. Only items with 20+ sales in 48 h qualify, so one fluke sale can't move the list.">Top movers</h3>
        <span class="exp">vs 90-day median · vol ≥ 20</span>
        <span class="grow"></span>
        <span class="lbl good">▲ Rising</span>
      </div>
      {@render miniTable(movers.risers, 'delta', false, 'No risers.')}
    </div>
    <div class="wrap tw movers">
      <div class="rail">
        <h3 class="sr-only">Top movers, falling</h3>
        <span class="grow"></span>
        <span class="lbl bad">▼ Falling</span>
      </div>
      {@render miniTable(movers.fallers, 'delta', false, 'No fallers.')}
    </div>
  </section>

  <!-- 3. VAULTED (2/3) + BARO (1/3) -->
  <section class="two-one">
    <div class="wrap tw vaulted">
      <div class="rail">
        <h3>Vaulted &amp; valuable</h3>
        <span class="exp">no longer drop, so supply is capped — high-value ones tend to hold or climb</span>
      </div>
      {@render miniTable(vaulted, 'avg', true, 'No vault data in this snapshot.')}
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
            Schedule only — bring your own ducats. His mods dip ~50% on arrival day; the money is in holding for the recovery.
          </p>
        </div>
        {#if baroStock.length}
          <table class="tw fixed">
            <colgroup><col /><col style="width:4rem" /><col style="width:4.5rem" /></colgroup>
            <thead><tr>
              <th class="l">{baroStockIsPast ? "Last visit's stock" : 'Stock'}</th>
              <th title="Ducat price at Baro">Ducats</th>
              <th title="Average of recent WFM sales">Avg now</th>
            </tr></thead>
            <tbody>
              {#each baroStock as s (s.name)}
                <tr>
                  <td class="l">{#if s.slug}<a href={wfmItemUrl(s.slug)} target="_blank" rel="noopener noreferrer">{s.name}</a>{:else}{s.name}{/if}</td>
                  <td>{#if s.ducats != null}<span class="ducat">{s.ducats}</span>{:else}<span class="faint">—</span>{/if}</td>
                  <td class="fg">{s.avg != null ? plat(s.avg) : '—'}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        {/if}
        {#if handoff}
          <div class="line"><a href="#desktop">Ducat math for what you own →</a></div>
        {/if}
      </div>
    {/if}
  </section>

  <!-- 4. RIVEN DISPOSITIONS — only when the snapshot carries changes -->
  {#if dispoChanges.length}
    <section class="wrap tw dispo" data-testid="dispo-changes">
      <div class="rail">
        <h3>Riven disposition changes</h3>
        <span class="exp">last 90 days · DE only raises dispositions now, so each change is a one-way price event for that weapon's rivens — WFM reprices within a day</span>
      </div>
      <table class="tw fixed">
        <colgroup><col /><col style="width:8rem" /><col style="width:4rem" /><col style="width:5rem" /></colgroup>
        <thead><tr>
          <th class="l">Weapon</th>
          <th title="Disposition before → after">Disposition</th>
          <th>Δ</th>
          <th title="When our scrape first saw the new value">Seen</th>
        </tr></thead>
        <tbody>
          {#each dispoChanges as c (c.slug + c.seen_at)}
            <tr>
              <td class="l">{c.name}</td>
              <td class:up={c.to > c.from} class:down={c.to < c.from} title={`Disposition ${c.from.toFixed(2)} → ${c.to.toFixed(2)}`}>{c.from.toFixed(2)} → <strong>{c.to.toFixed(2)}</strong></td>
              <td class:up={c.to > c.from} class:down={c.to < c.from}>{dispoDelta(c.from, c.to)}</td>
              <td>{seenDate(c.seen_at)}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    </section>
  {/if}

  <MetaDriftPanel {market} />

  <!-- 5. HAND-OFF: the same rows, completed by the desktop app (hosted only) -->
  {#if handoff}
    {@render handoff(sample)}
  {/if}
</section>

<style>
  .browser { display: flex; flex-direction: column; gap: var(--stack); min-width: 0; }
  .two { display: grid; grid-template-columns: 1fr 1fr; gap: var(--stack); min-width: 0; }
  .two-one { display: grid; grid-template-columns: 2fr 1fr; gap: var(--stack); min-width: 0; }
  @media (max-width: 900px) {
    .two, .two-one { grid-template-columns: 1fr; }
  }
  .sr-only { position: absolute; width: 1px; height: 1px; overflow: hidden; clip: rect(0 0 0 0); white-space: nowrap; }

  .lookup .bar .exp kbd {
    font-family: var(--font-mono);
    font-size: 10px;
    border: 1px solid var(--border);
    border-radius: var(--radius-tag);
    padding: 0 4px;
    color: var(--fg);
  }
  .lookup .exp.note { flex-shrink: 0; }
  .year-toggle.on { color: var(--fg); border-color: var(--accent); }
  .stale-note { color: var(--warn); }
  .thin-hist { font-size: 11px; color: var(--muted); font-family: var(--font-body); }
  /* Below the results table's natural width the panel pans sideways rather
     than squeezing the Item column. */
  .results { min-width: 56rem; }

  /* Baro card: rail glyph in ducat gold, mono clock. */
  .baro { display: flex; flex-direction: column; }
  .baro .glyph { color: var(--ducat); font-size: 14px; }
  .baro .body { display: flex; flex-direction: column; gap: var(--s2); }
  .baro .clock {
    font-family: var(--font-mono);
    font-size: 18px;
    line-height: 1.5rem;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
    color: var(--fg);
  }
  .baro .clock small {
    font: 600 10px/1rem var(--font-ui);
    color: var(--muted);
    letter-spacing: 0.1em;
    text-transform: uppercase;
    margin-right: var(--s2);
  }
  .baro .line { margin-top: auto; }
  .baro .line a { color: var(--accent); }
  .lookup .line a { color: var(--accent); }
</style>
