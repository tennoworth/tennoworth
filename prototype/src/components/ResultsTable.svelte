<script lang="ts">
  import { untrack } from 'svelte';

  // Row shape passed in from App.svelte's computeResults. Mirrors the
  // fields actually rendered/sorted; keep this in sync.
  interface Row {
    key?: string;
    slug: string;
    subtype: string | null;
    name: string;
    owned: number;
    type: string;
    kept_lvl: number | null;
    ducats: number | null;
    plat_per_100d: number | null;
    avg_price: number;
    low_sell: number;
    low5_avg: number;
    top_buy: number;
    volume_48h: number;
    ratio: number;
    potential_plat: number;
    raw_value: number;
    sell_score: number;
    patience: boolean;
    timing: 'hold' | 'peak' | 'neutral';
    medians_7d: number[];
    median_90d: number | null;
    delta_90d_pct: number | null;
    tags: string[];
    is_augment: boolean;
    vault_status: 'vaulted' | 'vaulting-soon' | 'available' | null;
  }

  interface ColumnDef {
    key: string;
    label: string;
    align: 'left' | 'right';
    noSort?: boolean;
  }

  interface Props {
    results: Row[];
    deltas?: Map<string, number>;
    visibleColumns?: string[] | null;
    presetSort?: { key: string; dir: number } | null;
  }
  let { results, deltas = new Map(), visibleColumns = null, presetSort = null }: Props = $props();

  let sortKey = $state<string>('sell_score');
  let sortDir = $state(-1);
  let filter = $state('');
  let pageSize = $state(20);
  let page = $state(0);

  // Pill filter — the badges rendered next to item names double as filterable
  // facets. Multi-select is OR ("show me peaks and holds"); empty = no filter.
  type PillKey = 'peak' | 'hold' | 'patience' | 'vaulted' | 'vaulting-soon' | 'aug';
  const PILL_DEFS: { key: PillKey; label: string; cls: string }[] = [
    { key: 'peak',          label: 'peak',          cls: 'peak' },
    { key: 'hold',          label: 'hold',          cls: 'hold' },
    { key: 'patience',      label: 'patience',      cls: 'patience' },
    { key: 'vaulted',       label: 'vaulted',       cls: 'vaulted' },
    { key: 'vaulting-soon', label: 'vaulting soon', cls: 'vaulting-soon' },
    { key: 'aug',           label: 'aug',           cls: 'augment' },
  ];
  let activePills = $state<Set<PillKey>>(new Set());

  function rowPills(r: Row): PillKey[] {
    const out: PillKey[] = [];
    if (r.timing === 'peak') out.push('peak');
    if (r.timing === 'hold') out.push('hold');
    if (r.patience) out.push('patience');
    if (r.vault_status === 'vaulted') out.push('vaulted');
    if (r.vault_status === 'vaulting-soon') out.push('vaulting-soon');
    if (r.is_augment) out.push('aug');
    return out;
  }

  function togglePill(key: PillKey): void {
    const next = new Set(activePills);
    if (next.has(key)) next.delete(key); else next.add(key);
    activePills = next;
    page = 0;
  }

  // Counts come from the un-pill-filtered rows so an active chip doesn't
  // zero out its siblings; chips with no matching rows aren't rendered.
  let pillCounts = $derived.by(() => {
    const counts = new Map<PillKey, number>();
    for (const r of results) {
      for (const p of rowPills(r)) counts.set(p, (counts.get(p) ?? 0) + 1);
    }
    return counts;
  });

  let openHelp = $state<string | null>(null);
  function toggleHelp(key: string, e: MouseEvent): void {
    e.stopPropagation();
    openHelp = openHelp === key ? null : key;
  }
  $effect(() => {
    if (!openHelp) return;
    const handler = (e: MouseEvent): void => {
      const t = e.target as HTMLElement | null;
      if (!t?.closest('.help-popover, .info-btn')) openHelp = null;
    };
    document.addEventListener('click', handler, true);
    return () => document.removeEventListener('click', handler, true);
  });

  let hasDeltas = $derived(deltas && deltas.size > 0);

  // Help text shown on hover (native title + dotted underline). Plain
  // language, no marketing — say what the number actually means.
  // Each help entry is {text, unit, dir} so the popover can render the
  // jargon-y bits explicitly (a casual-user persona reported reading the
  // column name and not knowing what direction was "good"). Falls back
  // to hover-tooltip for users who never click.
  interface HelpEntry { text: string; unit?: string; dir?: string; }
  const HELP: Record<string, HelpEntry> = {
    name:           { text: 'Display name on warframe.market. Click to open the listing.' },
    owned:          { text: 'How many copies you own in your inventory.', unit: 'count' },
    delta:          { text: 'Change in count vs. the previous inventory you loaded.', unit: 'count', dir: 'positive = farmed, negative = sold' },
    avg_price:      { text: 'Volume-weighted average across closed trades in the last 48 h.', unit: 'plat', dir: 'noisy on low-volume items' },
    low_sell:       { text: 'Lowest current sell listing from in-game / online players.', unit: 'plat', dir: 'what you can realistically clear at right now' },
    top_buy:        { text: 'Highest current buy offer from in-game / online players.', unit: 'plat', dir: 'instant-sell ceiling' },
    volume_48h:     { text: 'Trades closed in the last 48 h.', unit: 'trades / 48 h', dir: 'higher = more liquid; ≥ 5 is healthy' },
    ratio:          { text: 'Live buyers ÷ live sellers — a rough demand signal.', unit: 'ratio', dir: '> 1 = buyers outnumber sellers' },
    potential_plat: { text: 'Owned × Avg. Optimistic — selling N copies usually clears below the average.', unit: 'plat', dir: 'upper bound, not realistic' },
    raw_value:      { text: 'Owned × the average of the ~5 cheapest live asks (the highlighted @ price). What the stack is worth at current listings — no liquidity discount; one troll listing barely moves it.', unit: 'plat', dir: 'falls back to Owned × Avg until the next scrape adds ask-depth data' },
    sell_score:     { text: 'Expected plat per day if you listed everything. min(owned, vol_48h / 2) × low_sell. Items below 2 trades / 48 h get a "patience" tag instead.', unit: 'plat / day', dir: 'higher = better; uncapped' },
    ducats:         { text: 'Ducat value at Baro Ki’Teer.', unit: 'ducats', dir: 'only prime parts have a non-zero value' },
    plat_per_100d:  { text: 'Plat cost per 100 ducats of value. “Deal” badge fires below 20.', unit: 'plat / 100 ducats', dir: 'lower = better ducat trade than WFM' },
    medians_7d:     { text: 'Sparkline of the last 7 days of daily median price. Hover the line for the raw values.' },
    delta_90d_pct:  { text: 'Latest daily median vs the 90-day median.', unit: '%', dir: '▲ = price rising into a peak (sell now); ▼ = sliding' },
  };

  const ALL_COLUMNS: ColumnDef[] = [
    { key: 'name',           label: 'Item',     align: 'left'  },
    { key: 'owned',          label: 'Own',      align: 'right' },
    { key: 'delta',          label: 'Δ',        align: 'right' },
    { key: 'sell_score',     label: 'Score',    align: 'right' },
    { key: 'avg_price',      label: 'Avg',      align: 'right' },
    { key: 'low_sell',       label: 'Low sell', align: 'right' },
    { key: 'top_buy',        label: 'Top buy',  align: 'right' },
    { key: 'medians_7d',     label: 'Trend',    align: 'left',  noSort: true },
    { key: 'delta_90d_pct',  label: 'Δ 90d',    align: 'right' },
    { key: 'volume_48h',     label: 'Vol 48h',  align: 'right' },
    { key: 'ratio',          label: 'Demand',   align: 'right' },
    { key: 'ducats',         label: 'Ducats',   align: 'right' },
    { key: 'plat_per_100d',  label: 'p/100d',   align: 'right' },
    { key: 'raw_value',      label: 'Raw value', align: 'right' },
    { key: 'potential_plat', label: 'Potential', align: 'right' },
  ];

  // Ducat-deal threshold: anything below ~20p per 100 ducats is a row
  // where Baro is the better outlet than WFM (you net ≥ 5 ducats per
  // plat you'd otherwise lose). Junk primes in this band rarely move
  // on WFM anyway — clear them at Baro instead.
  const DUCAT_DEAL_THRESHOLD = 20.0;

  // Column filtering. (1) drop the delta column when no prior snapshot
  // is loaded — it'd be all zeros. (2) when a preset specifies a column
  // allow-list, narrow to it (preserving the preset's order so the
  // workflow's signal columns come first).
  let columns: ColumnDef[] = $derived.by(() => {
    let cols = hasDeltas ? ALL_COLUMNS : ALL_COLUMNS.filter((c) => c.key !== 'delta');
    if (Array.isArray(visibleColumns) && visibleColumns.length > 0) {
      const byKey = new Map(cols.map((c) => [c.key, c]));
      cols = visibleColumns.map((k) => byKey.get(k)).filter((c): c is ColumnDef => Boolean(c));
    }
    return cols;
  });

  // A preset can carry a default sort (the Ducats preset ranks by plat-per-100-
  // ducats ascending — best ducat trades first). presetSort changes identity
  // each time the active preset changes; apply it then. Writes go inside
  // untrack() so they don't re-trigger this effect, and a later user header
  // click (changes sortKey, not presetSort) is preserved until the next switch.
  $effect(() => {
    const ps = presetSort;
    if (!ps) return;
    untrack(() => { sortKey = ps.key; sortDir = ps.dir; });
  });

  // If the current sort column gets hidden by a preset switch, fall back to the
  // first visible sortable column. Tracks `columns`; reads/writes sortKey inside
  // untrack() so the fallback can't re-fire the effect — the old version read
  // AND wrote sortKey in one body, which the audit flagged as a loop risk.
  $effect(() => {
    const cols = columns;
    untrack(() => {
      if (!cols.find((c) => c.key === sortKey)) {
        const fallback = cols.find((c) => c.align === 'right' && !c.noSort);
        if (fallback) sortKey = fallback.key;
      }
    });
  });

  function setSort(key: string): void {
    const col = ALL_COLUMNS.find((c) => c.key === key);
    if (col?.noSort) return;
    if (sortKey === key) sortDir = -sortDir;
    else {
      sortKey = key;
      sortDir = ['name', 'category'].includes(key) ? 1 : -1;
    }
  }

  // Build SVG polyline points for an N-point sparkline. Normalises to a
  // fixed [1, H-1] band so a flat series doesn't render as a 0-height
  // line. Returns null when there aren't enough points to draw.
  function sparklinePoints(arr: number[] | null | undefined, w = 60, h = 18): string | null {
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

  function rowDelta(r: Row): number {
    // Deltas come from diffOwned which keys by the composite (slug|subtype)
    // so radiant vs intact relic counts don't collide. Each row carries its
    // own key already.
    return deltas.get(r.key ?? r.slug) ?? 0;
  }

  let sorted = $derived.by(() => {
    const f = filter.trim().toLowerCase();
    let rows = f
      ? results.filter((r) => (r.name || r.slug).toLowerCase().includes(f))
      : results;
    if (activePills.size > 0) {
      rows = rows.filter((r) => rowPills(r).some((p) => activePills.has(p)));
    }
    return [...rows].sort((a, b) => {
      const av = sortKey === 'delta' ? rowDelta(a) : (a as unknown as Record<string, unknown>)[sortKey];
      const bv = sortKey === 'delta' ? rowDelta(b) : (b as unknown as Record<string, unknown>)[sortKey];
      // Push nulls to the bottom regardless of sort direction — ducats /
      // p/100d are sparse, and a column full of "—" at the top is useless.
      if (av == null && bv == null) return 0;
      if (av == null) return 1;
      if (bv == null) return -1;
      if (typeof av === 'string' && typeof bv === 'string') return av.localeCompare(bv) * sortDir;
      return ((av as number) - (bv as number)) * sortDir;
    });
  });

  // Pagination — clamps current page when sorted/pageSize change so the
  // user doesn't end up on an empty trailing page after filtering.
  let maxPage = $derived(Math.max(0, Math.ceil(sorted.length / pageSize) - 1));
  let currentPage = $derived(Math.min(page, maxPage));
  let pageStart = $derived(currentPage * pageSize);
  let pageEnd = $derived(Math.min(pageStart + pageSize, sorted.length));
  let paged = $derived(sorted.slice(pageStart, pageEnd));

  function setPage(p: number): void {
    page = Math.max(0, Math.min(p, maxPage));
  }

  function fmt(v: unknown, key: string): string {
    if (v === null || v === undefined) return '—';
    if (typeof v === 'number') {
      if (key === 'ratio') return v.toFixed(2);
      if (key === 'plat_per_100d') return v.toFixed(1);
      if (key === 'avg_price' || key === 'potential_plat' || key === 'sell_score' || key === 'raw_value') return v.toFixed(0);
      return v.toLocaleString();
    }
    return String(v);
  }
</script>

<div class="wrap">
  <div class="toolbar">
    <input
      type="text"
      placeholder="Filter by name…"
      bind:value={filter}
      oninput={() => (page = 0)}
    />
    <div class="pill-filters">
      {#each PILL_DEFS as p (p.key)}
        {@const n = pillCounts.get(p.key) ?? 0}
        {#if n > 0 || activePills.has(p.key)}
          <button
            type="button"
            class="tag pill-chip {p.cls}"
            class:on={activePills.has(p.key)}
            onclick={() => togglePill(p.key)}
            title={activePills.has(p.key) ? 'Click to stop filtering by this badge' : `Show only rows tagged "${p.label}"`}
          >{p.label} <span class="pill-n">{n}</span></button>
        {/if}
      {/each}
    </div>
    <div class="muted">
      {sorted.length.toLocaleString()} rows · sorted by
      <strong>{columns.find((c) => c.key === sortKey)?.label}</strong>
      {sortDir === -1 ? '↓' : '↑'}
    </div>
  </div>

  <table>
    <thead>
      <tr>
        {#each columns as col}
          <th
            onclick={() => setSort(col.key)}
            class={col.align}
            class:active={sortKey === col.key}
            class:nosort={col.noSort}
            title={HELP[col.key]?.text}
          >
            <span class="hcontent">
              <span class="label">{col.label}</span>
              {#if HELP[col.key]}
                <button
                  type="button"
                  class="info-btn"
                  aria-label="What does {col.label} mean?"
                  onclick={(e) => toggleHelp(col.key, e)}
                >?</button>
                {#if openHelp === col.key}
                  <span class="help-popover" role="tooltip">
                    <span class="hp-text">{HELP[col.key].text}</span>
                    {#if HELP[col.key].unit}
                      <span class="hp-meta"><span class="hp-key">unit</span> {HELP[col.key].unit}</span>
                    {/if}
                    {#if HELP[col.key].dir}
                      <span class="hp-meta"><span class="hp-key">direction</span> {HELP[col.key].dir}</span>
                    {/if}
                  </span>
                {/if}
              {/if}
              {#if sortKey === col.key && !col.noSort}
                <span class="arrow">{sortDir === -1 ? '↓' : '↑'}</span>
              {/if}
            </span>
          </th>
        {/each}
      </tr>
    </thead>
    <tbody>
      {#each paged as r (r.key ?? r.slug)}
        {@const d = rowDelta(r)}
        <tr>
          {#each columns as col}
            <td class={col.align}>
              {#if col.key === 'name'}
                <a
                  href="https://warframe.market/items/{r.slug}"
                  target="_blank"
                  rel="noopener noreferrer"
                  >{r.name || r.slug}</a
                >
                {#if r.vault_status === 'vaulted'}
                  <span class="tag vaulted" title="Prime is currently vaulted. Listings often command a premium.">vaulted</span>
                {:else if r.vault_status === 'vaulting-soon'}
                  <span class="tag vaulting-soon" title="Estimated to vault within ~60 days. Selling now beats the post-vault floor for active traders.">vaulting soon</span>
                {/if}
                {#if r.is_augment}
                  <span class="tag augment" title="Syndicate augment mod. Typically 25,000 standing to re-purchase from the issuing syndicate (6 mainline syndicates).">aug</span>
                {/if}
                {#if r.patience}
                  <span class="tag patience" title="Volume under 2 trades/48h — listing will sit a while before clearing.">patience</span>
                {/if}
                {#if r.timing === 'hold'}
                  <span class="tag hold" title="Price is near its 90-day low — you'd be selling into a trough. Common right after a Baro visit floods the mod; it typically recovers over weeks. Consider holding.">hold</span>
                {:else if r.timing === 'peak'}
                  <span class="tag peak" title="Price is near its 90-day high — a good moment to list this one.">peak</span>
                {/if}
              {:else if col.key === 'delta'}
                {#if d > 0}
                  <span class="delta up">+{d}</span>
                {:else if d < 0}
                  <span class="delta down">{d}</span>
                {:else}
                  <span class="delta zero">·</span>
                {/if}
              {:else if col.key === 'ducats'}
                {#if r.ducats != null}
                  <span class="ducat-num">{r.ducats}</span>
                  {#if r.plat_per_100d != null && r.plat_per_100d <= DUCAT_DEAL_THRESHOLD}
                    <span class="ducat-badge" title="Listing's plat value is at or below 100 ducats — Baro is the better outlet for this row.">deal</span>
                  {/if}
                {:else}
                  <span class="muted">—</span>
                {/if}
              {:else if col.key === 'plat_per_100d'}
                {#if r.plat_per_100d != null}
                  <span class={r.plat_per_100d <= DUCAT_DEAL_THRESHOLD ? 'ducat-num' : ''}>{fmt(r.plat_per_100d, col.key)}</span>
                {:else}
                  <span class="muted">—</span>
                {/if}
              {:else if col.key === 'raw_value'}
                {#if r.raw_value > 0}
                  {fmt(r.raw_value, col.key)}
                  {#if r.low5_avg > 0}
                    <span class="ask-avg" title="Average of the ~5 cheapest live asks right now">@{fmt(r.low5_avg, 'plat_per_100d')}</span>
                  {/if}
                {:else}
                  <span class="muted">—</span>
                {/if}
              {:else if col.key === 'medians_7d'}
                {#if r.medians_7d && r.medians_7d.length >= 2}
                  <svg class="sparkline" viewBox="0 0 60 18" width="60" height="18" aria-hidden="true">
                    <title>last 7d medians: {r.medians_7d.join(', ')}</title>
                    <polyline points={sparklinePoints(r.medians_7d)} fill="none" stroke="currentColor" stroke-width="1.2" />
                  </svg>
                {:else}
                  <span class="muted">—</span>
                {/if}
              {:else if col.key === 'delta_90d_pct'}
                {#if r.delta_90d_pct == null}
                  <span class="muted">—</span>
                {:else if r.delta_90d_pct >= 1}
                  <span class="trend up" title="Latest median {r.delta_90d_pct.toFixed(0)}% above 90d median — sell into the peak">▲{r.delta_90d_pct.toFixed(0)}%</span>
                {:else if r.delta_90d_pct <= -1}
                  <span class="trend down" title="Latest median {Math.abs(r.delta_90d_pct).toFixed(0)}% below 90d median — price is sliding">▼{Math.abs(r.delta_90d_pct).toFixed(0)}%</span>
                {:else}
                  <span class="trend flat" title="Within ±1% of 90d median">·</span>
                {/if}
              {:else}
                {fmt((r as unknown as Record<string, unknown>)[col.key], col.key)}
              {/if}
            </td>
          {/each}
        </tr>
      {/each}
    </tbody>
  </table>

  {#if sorted.length > pageSize}
    <div class="pager">
      <div class="muted">
        Showing {(pageStart + 1).toLocaleString()}–{pageEnd.toLocaleString()}
        of {sorted.length.toLocaleString()}
      </div>
      <div class="pager-controls">
        <button
          class="page-btn"
          onclick={() => setPage(currentPage - 1)}
          disabled={currentPage === 0}
        >‹ Prev</button>
        <span class="muted">
          {currentPage + 1} / {maxPage + 1}
        </span>
        <button
          class="page-btn"
          onclick={() => setPage(currentPage + 1)}
          disabled={currentPage >= maxPage}
        >Next ›</button>
        <span class="pager-spacer"></span>
        <label class="page-size">
          Per page
          <select bind:value={pageSize}>
            <option value={20}>20</option>
            <option value={40}>40</option>
            <option value={60}>60</option>
            <option value={80}>80</option>
            <option value={100}>100</option>
          </select>
        </label>
      </div>
    </div>
  {/if}
</div>

<style>
  .wrap {
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 10px;
    /* Vertical: clip; Horizontal: scroll. Default `overflow: hidden`
       was clipping ~10 of 13 columns on tablet / phone widths — the
       table itself is intrinsically wide, so let the user scroll it
       sideways rather than amputating columns. */
    overflow-x: auto;
    overflow-y: hidden;
  }
  /* Keep the table from collapsing under flex / grid parents — without
     this `table { width: 100% }` shrinks the columns into ellipsis-soup
     instead of becoming scrollable. */
  table { min-width: max-content; }
  .toolbar {
    display: flex;
    gap: 12px;
    padding: 12px 14px;
    align-items: center;
    justify-content: space-between;
    border-bottom: 1px solid var(--border);
    flex-wrap: wrap;
  }
  .toolbar input { min-width: 260px; }
  /* Pill-filter chips reuse the badge palette (.tag.peak etc.) so the chip
     and the in-row pill it filters on read as the same object. */
  .pill-filters {
    display: flex;
    gap: 6px;
    flex-wrap: wrap;
    align-items: center;
    margin-right: auto;
  }
  .pill-chip {
    margin-left: 0;
    cursor: pointer;
    background: transparent;
    font-family: inherit;
    transition: background 120ms ease, border-color 120ms ease;
  }
  .pill-chip:hover { background: rgba(255,255,255,0.04); }
  .pill-chip.on {
    background: color-mix(in srgb, currentColor 14%, transparent);
    border-color: currentColor;
  }
  .pill-n {
    opacity: 0.65;
    font-size: 9px;
  }
  .muted { color: var(--muted); font-size: 12.5px; }
  table {
    width: 100%;
    border-collapse: collapse;
    font-variant-numeric: tabular-nums;
  }
  th, td {
    padding: 7px 12px;
    text-align: left;
    border-bottom: 1px solid var(--border);
  }
  th {
    background: var(--panel-2);
    font-weight: 600;
    cursor: pointer;
    user-select: none;
    position: sticky;
    top: 0;
  }
  th .hcontent {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    position: relative;
  }
  .info-btn {
    font-size: 9.5px;
    color: var(--muted);
    background: transparent;
    border: 1px solid var(--border);
    border-radius: 50%;
    width: 14px;
    height: 14px;
    padding: 0;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    line-height: 1;
    cursor: pointer;
    font-family: inherit;
    transition: color 120ms ease, border-color 120ms ease, background 120ms ease;
  }
  th:hover .info-btn,
  .info-btn:focus {
    color: var(--accent);
    border-color: var(--accent);
    outline: none;
  }
  /* Click-popover anchored under the `?` button. Stays inside the table
     visually but z-indexed above sticky headers; the click-outside
     listener in the script closes it. */
  .help-popover {
    position: absolute;
    top: calc(100% + 6px);
    left: 0;
    z-index: 50;
    width: 280px;
    background: var(--panel);
    border: 1px solid var(--accent);
    border-radius: 8px;
    padding: 10px 12px;
    box-shadow: 0 8px 24px rgba(0,0,0,0.45);
    font-size: 12px;
    font-weight: 400;
    color: var(--fg);
    line-height: 1.55;
    letter-spacing: 0;
    text-transform: none;
    display: flex;
    flex-direction: column;
    gap: 6px;
    white-space: normal;
    cursor: default;
  }
  th.right .help-popover { left: auto; right: 0; }
  .hp-text { color: var(--fg); }
  .hp-meta { color: var(--muted); font-size: 11px; display: flex; gap: 6px; }
  .hp-key {
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--accent);
    min-width: 60px;
    font-weight: 600;
  }
  th:hover { background: #232733; }
  th.right, td.right { text-align: right; }
  th.right .hcontent { justify-content: flex-end; }
  th.active { color: var(--accent); }
  tbody tr:hover { background: rgba(255,255,255,0.02); }
  td a { color: var(--fg); text-decoration: none; }
  td a:hover { color: var(--accent); text-decoration: underline; }
  .arrow { color: var(--accent); }
  .delta { font-weight: 500; }
  .delta.up   { color: var(--good); }
  .delta.down { color: var(--bad); }
  .delta.zero { color: var(--muted); }

  /* Sparkline + Δ-90d treatment. Sparkline uses currentColor stroked at
     1.2 px so it inherits the row colour; trend badge sits in its own
     column with directional colour. */
  th.nosort { cursor: default; }
  th.nosort:hover { background: var(--panel-2); }
  .sparkline {
    color: var(--accent);
    vertical-align: middle;
  }
  .trend {
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 11px;
    font-weight: 500;
  }
  .trend.up   { color: var(--good); }
  .trend.down { color: var(--bad); }
  .trend.flat { color: var(--muted); }

  /* Raw-value column: the total leads; the per-unit ask average rides
     along as the highlighted "@price" so the multiplier is auditable
     at a glance. */
  .ask-avg {
    margin-left: 5px;
    font-size: 11px;
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    color: var(--accent);
  }

  /* Ducat column treatment. Warm-gold tint on the number itself so the
     domain signal reads at a glance; a quiet "deal" badge rather than a
     row-level highlight (which would compete with sell_score sorting). */
  .ducat-num { color: var(--ducat); }
  .ducat-badge {
    display: inline-block;
    margin-left: 6px;
    padding: 0 5px;
    font-size: 9.5px;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    border: 1px solid color-mix(in srgb, var(--ducat) 40%, var(--border));
    color: var(--ducat);
    border-radius: 3px;
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    vertical-align: middle;
  }

  /* "patience" tag — quiet so it doesn't compete with the item name, but
     present enough that a scan picks it up. Used for items with vol_48h < 2,
     i.e. listings that exist but rarely clear. */
  .tag {
    display: inline-block;
    margin-left: 6px;
    padding: 1px 6px;
    font-size: 10px;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    border: 1px solid var(--border);
    border-radius: 4px;
    color: var(--muted);
    vertical-align: middle;
  }
  .tag.patience {
    color: var(--warn);
    border-color: color-mix(in srgb, var(--warn) 30%, var(--border));
  }
  /* Vault: a hard sell-signal for vaulted primes, lighter signal for
     vaulting-soon. Augment chip is informational only (re-buy cost). */
  .tag.vaulted {
    color: var(--bad);
    border-color: color-mix(in srgb, var(--bad) 35%, var(--border));
  }
  .tag.vaulting-soon {
    color: var(--warn);
    border-color: color-mix(in srgb, var(--warn) 35%, var(--border));
  }
  .tag.augment {
    color: var(--accent);
    border-color: color-mix(in srgb, var(--accent) 30%, var(--border));
  }
  /* Timing: "hold" warns you're near the 90d low (don't dump into a trough —
     e.g. a Baro-flooded mod); "peak" marks a price near its 90d high. */
  .tag.hold {
    color: var(--warn);
    border-color: color-mix(in srgb, var(--warn) 30%, var(--border));
  }
  .tag.peak {
    color: var(--good);
    border-color: color-mix(in srgb, var(--good) 35%, var(--border));
  }

  .pager {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 10px 14px;
    border-top: 1px solid var(--border);
    flex-wrap: wrap;
  }
  .pager-controls {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-wrap: wrap;
  }
  .pager-spacer { width: 8px; }
  .page-btn {
    background: var(--panel-2);
    border: 1px solid var(--border);
    color: var(--fg);
    font-size: 12px;
    padding: 4px 10px;
    border-radius: 6px;
    cursor: pointer;
    transition: color 120ms ease, border-color 120ms ease, background 120ms ease;
  }
  .page-btn:hover:not(:disabled) {
    color: var(--accent);
    border-color: var(--accent);
  }
  .page-btn:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }
  .page-size {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    color: var(--muted);
    font-size: 12px;
  }
  .page-size select {
    font: inherit;
    color: var(--fg);
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 3px 6px;
  }
</style>
