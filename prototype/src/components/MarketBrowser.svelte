<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import type { Market } from '../lib/types';
  import {
    buildBrowseIndex,
    searchItems,
    topMovers,
    vaultedTop,
    type BrowseRow,
  } from '../lib/market-browse';

  // Powered ONLY by the already-loaded market.json — no fetches here. App.svelte
  // passes the snapshot plus its own freshness/staleness derivations so the
  // status line stays a single source of truth with the dashboard.
  let {
    market,
    staleness = null,
    freshness = 'unknown',
  }: {
    market: Market;
    staleness?: string | null;
    freshness?: 'fresh' | 'aging' | 'stale' | 'unknown';
  } = $props();

  let query = $state('');

  // Index + the standing reports are pure derivations of the snapshot.
  let index = $derived(buildBrowseIndex(market));
  let results = $derived(searchItems(market, index, query, 12));
  let movers = $derived(topMovers(market, index, { minVol: 20, minPrice: 10, limit: 8 }));
  let vaulted = $derived(vaultedTop(market, index, 12));

  // Baro schedule — same NODE_NAMES cleanup the dashboard applies. Schedule
  // only: market.json carries activation/expiry/location, never stock.
  const NODE_NAMES: Record<string, string> = {
    TennoConHUB2: 'TennoCon Relay',
    SolarisUnitedHub1: 'Fortuna backroom',
  };
  let baro = $derived.by(() => {
    const b = market?.baro;
    if (!b) return null;
    return { ...b, location: NODE_NAMES[b.location] ?? b.location };
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

  function humanWindow(ms: number | null): string {
    if (ms == null || !Number.isFinite(ms) || ms < 0) return '—';
    const totalMin = Math.floor(ms / 60000);
    const d = Math.floor(totalMin / (60 * 24));
    const h = Math.floor((totalMin / 60) % 24);
    const m = totalMin % 60;
    if (d > 0) return `${d}d ${h}h`;
    if (h > 0) return `${h}h ${m}m`;
    return `${m}m`;
  }

  // Normalise a 7-point series into a fixed [1, H-1] band so a flat line
  // still draws. Mirrors ResultsTable's sparkline. null when too few points.
  function sparklinePoints(arr: number[] | null | undefined, w = 56, h = 16): string | null {
    if (!Array.isArray(arr) || arr.length < 2) return null;
    let min = Infinity, max = -Infinity;
    for (const v of arr) {
      if (v < min) min = v;
      if (v > max) max = v;
    }
    const range = max - min || 1;
    const step = w / (arr.length - 1);
    return arr.map((v, i) => {
      const x = i * step;
      const y = (h - 1) - ((v - min) / range) * (h - 2);
      return `${x.toFixed(1)},${y.toFixed(1)}`;
    }).join(' ');
  }

  const plat = (v: number) => Math.round(v).toLocaleString();
</script>

<section class="browser" data-testid="market-browser">
  <div class="browser-head">
    <h2>What's worth selling right now</h2>
    <span class="market-status">
      <span class="dot {freshness}" role="img" aria-label="Market data {freshness}"></span>
      Market snapshot {staleness ?? '—'}{freshness !== 'unknown' ? ` · ${freshness}` : ''}
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
      {#if r.deltaPct != null && Math.abs(r.deltaPct) >= 1}
        {#if r.deltaPct > 0}
          <span class="trend up" title="Latest median {r.deltaPct.toFixed(0)}% above the 90-day median">▲{r.deltaPct.toFixed(0)}%</span>
        {:else}
          <span class="trend down" title="Latest median {Math.abs(r.deltaPct).toFixed(0)}% below the 90-day median">▼{Math.abs(r.deltaPct).toFixed(0)}%</span>
        {/if}
      {/if}
      {#if sparklinePoints(r.medians_7d)}
        <svg class="sparkline" viewBox="0 0 56 16" width="56" height="16" aria-hidden="true">
          <title>7-day medians: {r.medians_7d?.join(', ')}</title>
          <polyline points={sparklinePoints(r.medians_7d)} fill="none" stroke="currentColor" stroke-width="1.5" />
        </svg>
      {/if}
      <span class="price">{plat(r.avg)}<span class="unit">p</span></span>
      <span class="vol" title="48-hour trade volume">{r.vol.toLocaleString()}<span class="unit">/48h</span></span>
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
    <h3>Top movers <span class="muted">· vs 90-day median · vol ≥ 20</span></h3>
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
  .dot.fresh { background: var(--good); box-shadow: 0 0 6px color-mix(in srgb, var(--good) 60%, transparent); }
  .dot.aging { background: var(--warn); }
  .dot.stale { background: var(--bad); }
  .stale-note { margin: 0; color: var(--warn); font-size: 12.5px; }

  .card {
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 10px;
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
    padding: 5px 0;
    border-top: 1px solid var(--border);
    font-size: 13px;
  }
  .item:first-child { border-top: none; }
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

  .trend {
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 11px;
    font-weight: 500;
    white-space: nowrap;
    flex-shrink: 0;
  }
  .trend.up { color: var(--good); }
  .trend.down { color: var(--bad); }

  /* Understated text tag, not a glow-pill — matches the repo's badge treatment. */
  .vault-badge {
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 10px;
    border: 1px solid var(--border);
    border-radius: 3px;
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
</style>
