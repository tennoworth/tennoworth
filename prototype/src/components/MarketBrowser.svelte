<script lang="ts">
  import { baroLocation, humanWindow, plat, freshnessLabel } from '../lib/format';
  import { onMount, onDestroy } from 'svelte';
  import type { Market } from '../lib/types';
  import {
    buildBrowseIndex,
    searchItems,
    topMovers,
    vaultedTop,
    dispositionChanges,
    type BrowseRow,
  } from '../lib/market-browse';
  import { sparklinePoints } from '../lib/sparkline';
  import { weekly, yearStats, type History } from '../lib/history';

  // Powered by the already-loaded market.json — the only fetch this component
  // can trigger is the optional year-long history, and only when the user
  // flips the "1 year" toggle (App passes the transport's loader so the
  // desktop keeps its egress in Rust).
  let {
    market,
    staleness = null,
    freshness = 'unknown',
    loadHistory = null,
  }: {
    market: Market;
    staleness?: string | null;
    freshness?: 'fresh' | 'aging' | 'stale' | 'unknown';
    loadHistory?: (() => Promise<History | null>) | null;
  } = $props();

  let query = $state('');

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

  // Index + the standing reports are pure derivations of the snapshot.
  let index = $derived(buildBrowseIndex(market));
  let results = $derived(searchItems(market, index, query, 12));
  let movers = $derived(topMovers(market, index, { minVol: 20, minPrice: 10, limit: 8 }));
  let vaulted = $derived(vaultedTop(market, index, 12));
  let dispoChanges = $derived(dispositionChanges(market, 12));
  function dispoDelta(from: number, to: number): string {
    const d = to - from;
    return `${d > 0 ? '+' : ''}${d.toFixed(2)}`;
  }
  function seenDate(iso: string): string {
    const t = Date.parse(iso);
    return Number.isFinite(t) ? new Date(t).toLocaleDateString(undefined, { month: 'short', day: 'numeric' }) : '';
  }

  // Baro schedule. Schedule only: market.json carries activation/expiry/
  // location, never stock.
  let baro = $derived.by(() => {
    const b = market?.baro;
    if (!b) return null;
    return { ...b, location: baroLocation(b.location) };
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
      return { phase: 'here' as const, label: 'Baro leaves in', windowMs: exp - now };
    }
    if (Number.isFinite(arr) && now < arr) {
      return { phase: 'incoming' as const, label: 'Baro arrives in', windowMs: arr - now };
    }
    return { phase: 'unknown' as const, label: 'Next Baro visit', windowMs: null };
  });


</script>

<section class="browser" data-testid="market-browser">
  <div class="browser-head">
    <h2>What's worth selling right now</h2>
    <span class="market-status">
      <span class="dot {freshness}" role="img" aria-label="Market data: {freshnessLabel(freshness)}" title={freshnessLabel(freshness)}></span>
      Market snapshot {staleness ?? '—'}{freshness !== 'unknown' ? ` · ${freshness}` : ''}
      {#if loadHistory}
        <button class="year-toggle" class:on={showYear} onclick={toggleYear} aria-pressed={showYear}
          title="Show each item's last year of daily prices (relics.run archive) instead of the last 7 days">
          {histState === 'loading' ? 'Loading 1 year…' : '1 year'}
        </button>
        {#if showYear && histState === 'unavailable'}<span class="muted">· history unavailable right now</span>{/if}
        {#if showYear && histState === 'ready' && history?.through}<span class="muted">· through {history.through}</span>{/if}
      {/if}
    </span>
  </div>

  {#if freshness === 'stale'}
    <p class="stale-note">⚠ This snapshot is {staleness} old — prices below may lag the live book.</p>
  {/if}

  {#snippet row(r: BrowseRow)}
    <div class="item">
      <span class="nm" title={r.name}>{r.name}</span>
      {#if r.vault === 'vaulted'}
        <span class="vault-badge vaulted" title="Vaulted — no longer dropping, supply is capped">vaulted</span>
      {:else if r.vault === 'vaulting-soon'}
        <span class="vault-badge soon" title="Vaulting soon — supply about to be capped">soon</span>
      {/if}
      {#if showYear && histState === 'ready'}
        {@const y = yearFor(r.slug)}
        {#if y}
          {#if y.stats.deltaPct != null && Math.abs(y.stats.deltaPct) >= 1}
            {#if y.stats.deltaPct > 0}
              <span class="trend up" title="Latest daily median {y.stats.deltaPct.toFixed(0)}% above where it was a year ago ({y.stats.baseline}p → {y.stats.latest}p; year low {y.stats.low}p, high {y.stats.high}p)">▲{y.stats.deltaPct.toFixed(0)}% 1y</span>
            {:else}
              <span class="trend down" title="Latest daily median {Math.abs(y.stats.deltaPct).toFixed(0)}% below where it was a year ago ({y.stats.baseline}p → {y.stats.latest}p; year low {y.stats.low}p, high {y.stats.high}p)">▼{Math.abs(y.stats.deltaPct).toFixed(0)}% 1y</span>
            {/if}
          {/if}
          {#if sparklinePoints(y.spark, 84, 16)}
            <svg class="sparkline year" viewBox="0 0 84 16" width="84" height="16" aria-hidden="true">
              <title>Weekly medians over the last year: low {y.stats.low}p, high {y.stats.high}p, {y.stats.tradedDays} traded days</title>
              <polyline points={sparklinePoints(y.spark, 84, 16)} fill="none" stroke="currentColor" stroke-width="1.5" />
            </svg>
          {/if}
        {:else}
          <span class="muted thin" title="Fewer than 20 traded days in the last year">thin history</span>
        {/if}
      {:else}
        {#if r.deltaPct != null && Math.abs(r.deltaPct) >= 1}
          {#if r.deltaPct > 0}
            <span class="trend up" title="Latest median {r.deltaPct.toFixed(0)}% above the 90-day median">▲{r.deltaPct.toFixed(0)}%</span>
          {:else}
            <span class="trend down" title="Latest median {Math.abs(r.deltaPct).toFixed(0)}% below the 90-day median">▼{Math.abs(r.deltaPct).toFixed(0)}%</span>
          {/if}
        {/if}
        {#if sparklinePoints(r.medians_7d, 56, 16)}
          <svg class="sparkline" viewBox="0 0 56 16" width="56" height="16" aria-hidden="true">
            <title>7-day medians: {r.medians_7d?.join(', ')}</title>
            <polyline points={sparklinePoints(r.medians_7d, 56, 16)} fill="none" stroke="currentColor" stroke-width="1.5" />
          </svg>
        {/if}
      {/if}
      <span class="price" title="Average of recent WFM sales — list below it to sell faster">{plat(r.avg)}<span class="unit">p</span></span>
      <span class="vol" title="Trades completed in the last 48 hours">{r.vol.toLocaleString()}<span class="unit">/48h</span></span>
    </div>
  {/snippet}

  <div class="search card">
    <input
      type="text"
      placeholder="Search any item — try “primed”, “mag”, “ash prime set”…"
      bind:value={query}
      aria-label="Search items"
    />
    {#if query.trim()}
      {#if results.length}
        <div class="list">
          {#each results as r (r.slug)}{@render row(r)}{/each}
        </div>
      {:else}
        <p class="muted empty">No priceable items match “{query.trim()}”.</p>
      {/if}
    {:else}
      <p class="muted hint">Start typing to look up any tradeable item's price, 48h volume and 7-day trend.</p>
    {/if}
  </div>

  <div class="card movers">
    <h3 title="Compares the latest price to the 90-day median. Only items with 20+ sales in 48 h qualify, so one fluke sale can't move the list.">Top movers <span class="muted">· vs 90-day median · vol ≥ 20</span></h3>
    <div class="cols">
      <div class="col">
        <div class="col-label up">Rising</div>
        {#if movers.risers.length}
          <div class="list">{#each movers.risers as r (r.slug)}{@render row(r)}{/each}</div>
        {:else}
          <p class="muted empty">No risers.</p>
        {/if}
      </div>
      <div class="col">
        <div class="col-label down">Falling</div>
        {#if movers.fallers.length}
          <div class="list">{#each movers.fallers as r (r.slug)}{@render row(r)}{/each}</div>
        {:else}
          <p class="muted empty">No fallers.</p>
        {/if}
      </div>
    </div>
  </div>

  <div class="card vaulted">
    <h3>Vaulted &amp; valuable</h3>
    <p class="muted lead">Vaulted items no longer drop, so supply is capped — the high-value ones tend to hold or climb.</p>
    {#if vaulted.length}
      <div class="list two-col">{#each vaulted as r (r.slug)}{@render row(r)}{/each}</div>
    {:else}
      <p class="muted empty">No vault data in this snapshot.</p>
    {/if}
  </div>

  {#if dispoChanges.length}
    <div class="card dispo" data-testid="dispo-changes">
      <h3>Riven disposition changes <span class="muted">· last 90 days</span></h3>
      <p class="muted lead">DE now only raises dispositions, so each change is a one-way price event for that weapon's rivens — and WFM reprices within a day. Dates are when our scrape first saw the new value.</p>
      <div class="list two-col">
        {#each dispoChanges as c (c.slug + c.seen_at)}
          <div class="row dispo-row">
            <span class="name">{c.name}</span>
            <span class="dispo-move" class:up={c.to > c.from} class:down={c.to < c.from} title={`Disposition ${c.from.toFixed(2)} → ${c.to.toFixed(2)}`}>
              {c.from.toFixed(2)} → <strong>{c.to.toFixed(2)}</strong> ({dispoDelta(c.from, c.to)})
            </span>
            <span class="muted seen">{seenDate(c.seen_at)}</span>
          </div>
        {/each}
      </div>
    </div>
  {/if}

  {#if baro && baroState}
    <div class="card baro">
      <span class="baro-icon" aria-hidden="true">⌬</span>
      <div class="baro-body">
        <div class="baro-clock">
          <span class="baro-label">{baroState.label}</span>
          <strong class="baro-val">{humanWindow(baroState.windowMs)}</strong>
        </div>
        <p class="muted">
          {#if baroState.phase === 'here'}
            Baro Ki'Teer is at {baro.location} now.
          {:else if baroState.phase === 'incoming'}
            Baro Ki'Teer arrives at {baro.location}.
          {:else}
            Next visit at {baro.location}.
          {/if}
          Schedule only — bring your own ducats.
        </p>
      </div>
    </div>
  {/if}
</section>

<style>
  .browser { display: flex; flex-direction: column; gap: 14px; }

  .browser-head {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 12px;
    flex-wrap: wrap;
  }
  .browser-head h2 {
    margin: 0;
    font-size: 14px;
    font-weight: 600;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: var(--muted);
  }
  .market-status {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    color: var(--muted);
    font-size: 12px;
    font-variant-numeric: tabular-nums;
  }
  /* Freshness dot — same green/amber/red scale as the dashboard stats strip. */
  .dot { width: 7px; height: 7px; border-radius: 50%; background: var(--muted); display: inline-block; }
  .dot.fresh { background: var(--good); box-shadow: var(--glow); }
  .dot.aging { background: var(--warn); }
  .dot.stale { background: var(--bad); }
  .stale-note { margin: 0; color: var(--warn); font-size: 12.5px; }

  .card {
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: var(--radius-panel);
    padding: 14px 16px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  h3 { margin: 0; font-size: 13px; font-weight: 600; }
  h3 .muted { font-weight: 400; }
  .muted { color: var(--muted); font-size: 12px; }
  .lead { margin: 0; }
  .empty { margin: 4px 0 0 0; }
  .hint { margin: 8px 0 0 0; }

  .search input { width: 100%; }

  /* Rising | Falling. min-width:0 lets the nowrap item names ellipsis instead
     of forcing the track wider than its share (the grid-overflow gotcha). */
  .movers .cols { display: grid; grid-template-columns: 1fr 1fr; gap: 16px 24px; }
  .movers .col { min-width: 0; }
  .col-label {
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    margin-bottom: 6px;
  }
  .col-label.up { color: var(--good); }
  .col-label.down { color: var(--bad); }

  .list { display: flex; flex-direction: column; }
  /* Vaulted items pack two-up on wide screens to use the horizontal space. */
  .list.two-col { display: grid; grid-template-columns: 1fr 1fr; gap: 0 24px; }
  .list.two-col > .item { min-width: 0; }
  .list.two-col > .item:nth-child(-n + 2) { border-top: none; }
  @media (max-width: 620px) {
    .movers .cols { grid-template-columns: 1fr; }
    .list.two-col { grid-template-columns: 1fr; }
  }

  .item {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 5px 6px;
    margin: 0 -6px;
    border-top: 1px var(--rule) var(--hairline);
    border-radius: var(--radius-input);
    font-size: 13px;
  }
  .item:first-child { border-top: none; }
  .item:hover { background: var(--panel-2); }
  .nm {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .price {
    font-variant-numeric: tabular-nums;
    font-weight: 600;
    white-space: nowrap;
  }
  .vol {
    font-variant-numeric: tabular-nums;
    color: var(--muted);
    white-space: nowrap;
    min-width: 62px;
    text-align: right;
  }
  .unit { color: var(--muted); font-size: 10px; margin-left: 1px; }

  .sparkline { color: var(--accent); flex-shrink: 0; vertical-align: middle; opacity: 0.85; }
  .sparkline.year { opacity: 0.95; }
  .thin { font-size: 11px; }
  .year-toggle {
    margin-left: 8px;
    background: transparent;
    color: var(--muted);
    border: 1px solid var(--border);
    border-radius: var(--radius-ctl);
    padding: 1px 8px;
    font-size: 11px;
    cursor: pointer;
    font: inherit;
  }
  .year-toggle.on { color: var(--fg); border-color: var(--accent); }
  .year-toggle:hover { color: var(--fg); }

  .trend {
    font-family: var(--font-mono);
    font-size: 11px;
    font-weight: 500;
    white-space: nowrap;
    flex-shrink: 0;
  }
  .trend.up { color: var(--good); }
  .trend.down { color: var(--bad); }

  /* Understated text tag, not a glow-pill — matches the repo's badge treatment. */
  .vault-badge {
    font-family: var(--font-mono);
    font-size: 10px;
    border: 1px solid var(--border);
    border-radius: var(--radius-tag);
    padding: 0 5px;
    white-space: nowrap;
    flex-shrink: 0;
  }
  .vault-badge.vaulted { color: var(--warn); border-color: color-mix(in srgb, var(--warn) 40%, var(--border)); }
  .vault-badge.soon { color: var(--accent); border-color: color-mix(in srgb, var(--accent) 40%, var(--border)); }

  .baro { flex-direction: row; align-items: center; gap: 14px; }
  .baro-icon { font-size: 22px; color: var(--ducat); }
  .baro-body { display: flex; flex-direction: column; gap: 2px; }
  .baro-clock { display: flex; align-items: baseline; gap: 8px; }
  .baro-label { font-size: 11px; letter-spacing: 0.04em; text-transform: uppercase; color: var(--muted); }
  .baro-val { font-size: 16px; font-weight: 600; font-variant-numeric: tabular-nums; }
  .baro-body p { margin: 0; }
  .dispo-row { display: flex; align-items: baseline; gap: 10px; }
  .dispo-row .name { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .dispo-move { font-family: var(--font-mono); font-size: 12px; white-space: nowrap; }
  .dispo-move.up { color: var(--good); }
  .dispo-move.down { color: var(--bad); }
  .dispo-row .seen { font-size: 11px; white-space: nowrap; }
</style>
