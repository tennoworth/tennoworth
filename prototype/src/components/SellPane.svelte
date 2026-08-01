<script lang="ts">
  // @ts-nocheck — presentation glue (event handlers, derived display strings),
  // same rationale as App.svelte's own @ts-nocheck.
  //
  // The Sell view: stats strip, Top Picks, the preset/tag-chip/filter
  // toolbar, the score explainer, and ResultsTable itself. The single
  // biggest slice of the 2026-07-24 App.svelte god-object split.
  //
  // Ownership split: the raw filter primitives (minPrice/minOwned/
  // typeFilter/hideAtLvl/activeTags) are $bindable — App.svelte's own
  // filterState $derived and the results-recompute $effect need to read
  // them, so App.svelte owns the real $state and this component both
  // displays and writes them through native bind:value. tableView is
  // $bindable for the same reason (AssistantChat reads it). Everything
  // else here (allPicks/picks/snoozedPicks, pickReason, toggleTag,
  // relaxFilters) is local — nothing outside this view ever touched them.
  import ResultsTable from './ResultsTable.svelte';
  import { PRESETS } from '../lib/presets';
  import { wfmItemUrl, plat } from '../lib/format';
  import { selectPicks, MIN_PICK_SCORE } from '../lib/sell-priority';

  let {
    minPrice = $bindable(),
    minOwned = $bindable(),
    typeFilter = $bindable(),
    hideAtLvl = $bindable(),
    activeTags = $bindable(),
    tableView = $bindable(),

    resolved, results, deltas, totalPotential,
    marketFreshness, marketStaleness, marketLoadError,
    listableRows, availableTags, availableTypes,
    visibleColumns, presetSort, emptyReason,
    activePreset, reserveCopies, filtersOpen, scoreExplainerDismissed,
    isDesktop, companionStatus,

    applyPreset, setReserveCopies, toggleFiltersOpen, dismissScoreExplainer,
    openListingFlow,
    pendingBanner,
  } = $props();

  // Was inline in the template, so it re-filtered the whole results array on
  // EVERY render of this pane — including every keystroke in the name filter,
  // which cannot change the answer. $derived recomputes only when results does.
  let sellableCount = $derived(results.filter((r) => r.sellable > 0).length);

  // Auto-derived options for the type dropdown label map. The Type filter
  // mixes warframestat catalog categories (Mods, Arcanes, Relics…) with raw
  // inventory.json keys used as fallback (MiscItems, RawUpgrades…). The
  // internal names leak as-is without this map.
  const TYPE_LABELS = {
    MiscItems: 'Parts & misc',
    Recipes: 'Blueprints',
    RawUpgrades: 'Mods (unranked stacks)',
    Suits: 'Warframes',
    LongGuns: 'Primary weapons',
    Pistols: 'Secondary weapons',
    Melee: 'Melee weapons',
    SpaceGuns: 'Archwing guns',
    SpaceMelee: 'Archwing melee',
    SentinelWeapons: 'Sentinel weapons',
  };

  function toggleTag(tag) {
    const next = new Set(activeTags);
    if (next.has(tag)) next.delete(tag); else next.add(tag);
    activeTags = next;
  }

  // Top picks strip. Built from `results`, which is already sorted best-first,
  // so selectPicks is a cheap filter+slice — no rescoring. Deliberately reads
  // `results` rather than `tableView.rows`: the table's own local name-search
  // box and pill-filter chips only narrow `tableView.rows`, so picks stay
  // "global" with respect to those. They are NOT independent of the filter
  // rail / active preset, though — App.svelte's computeResults() reads
  // minPrice/minOwned/typeFilter/hideAtLvl/activeTags and the active
  // preset's own floors (vaultOnly/ducatsOnly/minVol/minMedian) directly
  // rather than taking them as parameters, so `results` (and therefore
  // picks) narrows along with whatever preset the user has selected. True
  // preset-independence would need computeResults to accept filter
  // overrides — out of scope here.
  let allPicks = $derived.by(() => selectPicks(results));
  // Snooze is session-only, no persistence and no new localStorage key — a
  // dismissed pick reappearing on reload is the honest, cheap behaviour; it
  // isn't worth a storage-key version bump for a "hide until refresh" nicety.
  let snoozedPicks = $state(new Set());
  let picks = $derived(allPicks.filter((p) => !snoozedPicks.has(p.key ?? p.slug)));
  function snoozePick(key) {
    const next = new Set(snoozedPicks);
    next.add(key);
    snoozedPicks = next;
  }

  // Plain-language reason line for a pick, built only from fields the row
  // already carries (timing / delta_90d_pct / clearing_price / volume_48h) —
  // no invented ETAs or destinations. Order of checks mirrors how confident
  // each signal is: a corroborated peak is the strongest "act now" case,
  // hold is the strongest "don't" case, then whatever 90d trend exists,
  // then the plain fallback for a flat/illiquid-trend row.
  function pickReason(p) {
    const price = Math.round(p.clearing_price);
    if (p.timing === 'hold') {
      return 'Near its 90-day low — selling now leaves plat on the table.';
    }
    if (p.timing === 'peak') {
      return p.delta_90d_pct != null && p.delta_90d_pct > 0
        ? `Near its 90-day high, up ${Math.round(p.delta_90d_pct)}% — a good moment to list around ${price}p.`
        : `Near its 90-day high — a good moment to list around ${price}p.`;
    }
    if (p.delta_90d_pct != null && p.delta_90d_pct >= 1) {
      return `Up ${Math.round(p.delta_90d_pct)}% vs its 90-day baseline — clears around ${price}p at current demand.`;
    }
    if (p.delta_90d_pct != null && p.delta_90d_pct <= -1) {
      return `Down ${Math.abs(Math.round(p.delta_90d_pct))}% vs its 90-day baseline — clears around ${price}p at current demand.`;
    }
    return `Clears around ${price}p at current demand.`;
  }

  // Preset-driven empty states reset the preset; hand-filter ones relax the
  // offending slider. A price slider can't rescue a "no vaulted parts" empty.
  const PRESET_EMPTY_KINDS = new Set(['tag', 'vault', 'ducats', 'vol', 'median']);
  // One-shot quick-fix actions the empty state can offer.
  function relaxFilters({ kind }) {
    if (kind === 'price') minPrice = 1;
    if (kind === 'owned') minOwned = 1;
    if (kind === 'type')  typeFilter = 'all';
    if (kind === 'kept')  hideAtLvl = 11;
    if (PRESET_EMPTY_KINDS.has(kind)) applyPreset('default');
  }
</script>

<section class="view-header">
  <h2>Sell</h2>
  <span
    class="lede-dot"
    role="img"
    aria-label="About this view"
    title="Items in your inventory worth listing right now, ranked by sell score."
  >ⓘ</span>
</section>

<section class="stats">
  <div class="stat">
    <span class="k">Owned</span>
    <span class="v">{resolved.owned.size.toLocaleString()}</span>
  </div>
  <div class="stat">
    <span class="k">Sellable</span>
    <span class="v">{sellableCount.toLocaleString()}</span>
  </div>
  <div class="stat">
    <span class="k">Potential</span>
    <span class="v">
      {totalPotential.toLocaleString(undefined, { maximumFractionDigits: 0 })}
      <span class="unit">p</span>
    </span>
  </div>
  <div class="stat right">
    <span class="k">
      <span class="dot {marketFreshness}" role="img" aria-label="Market data {marketFreshness}"></span>
      Market data
    </span>
    <span class="v small">{marketStaleness ?? '—'}{marketFreshness !== 'unknown' ? ` · ${marketFreshness}` : ''}</span>
  </div>
</section>

{#if marketLoadError}
  <div class="card warn-banner">⚠ {marketLoadError}</div>
{:else if marketFreshness === 'stale'}
  <div class="card warn-banner">
    ⚠ Prices may be outdated — this market snapshot is {marketStaleness} old. Rankings below use stale data.
  </div>
{/if}

{#if results.length > 0}
  {#if allPicks.length > 0}
    <section class="card picks">
      <div class="picks-head">
        <div class="picks-title">
          <h3>Top picks</h3>
          <span
            class="muted"
            title="Same Score as the table below — expected plat per day if listed. Picks also need at least 3 trades/48h (so a listing won't just sit) and {MIN_PICK_SCORE}p/day to clear the bar."
          >Best sells right now, ranked by expected plat/day — thin-volume listings excluded.</span>
        </div>
      </div>
      <div class="picks-body">
        {#each picks as p, i (p.key ?? p.slug)}
          <div class="pick-row">
            <span class="pick-rank">{i + 1}</span>
            <div class="pick-main">
              <a
                class="pick-name"
                href={wfmItemUrl(p.slug)}
                target="_blank"
                rel="noopener noreferrer"
              >{p.name}</a>
              <p class="pick-reason" class:hold={p.timing === 'hold'} class:peak={p.timing === 'peak'}>
                {pickReason(p)}
                <span class="pick-vol">({plat(p.volume_48h)} trades / 48h)</span>
              </p>
            </div>
            <div class="pick-score">~{plat(p.sell_score)}<span class="unit">p/day</span></div>
            <div class="pick-actions">
              <button class="pick-list" onclick={() => openListingFlow(p)} aria-label="List {p.name} on WFM">List</button>
              <button
                type="button"
                class="pick-snooze"
                onclick={() => snoozePick(p.key ?? p.slug)}
                aria-label="Hide {p.name} for this session"
                title="Hide for this session"
              >×</button>
            </div>
          </div>
        {/each}
        {#if picks.length === 0}
          <p class="muted picks-all-snoozed">All picks snoozed for this session.</p>
        {/if}
      </div>
    </section>
  {:else}
    <div class="card empty">
      <div>
        <strong>No picks clear the bar right now.</strong>
        <p class="muted">Nothing owned trades often enough and scores high enough to headline — check the table below for the rest.</p>
      </div>
    </div>
  {/if}
{/if}

<section class="sell-toolbar">
  <div class="toolbar-group presets-row">
    {#each Object.entries(PRESETS) as [name, preset]}
      <button
        type="button"
        class="preset"
        class:active={activePreset === name}
        aria-pressed={activePreset === name}
        onclick={() => applyPreset(name)}
        title={preset.hint}
      >{preset.label}</button>
    {/each}
  </div>
  <span class="muted preset-hint">
    {activePreset ? PRESETS[activePreset].hint : 'custom — saved preset cleared'}
  </span>
  {#if availableTags.length > 0}
    <div class="toolbar-divider" aria-hidden="true"></div>
    <div class="toolbar-group tagchips">
      {#each availableTags as [tag, count]}
        <button
          type="button"
          class="chip"
          class:active={activeTags.has(tag)}
          class:zero={count === 0}
          aria-pressed={activeTags.has(tag)}
          onclick={() => toggleTag(tag)}
          title={count === 0 ? `No matching rows pass the other filters` : `${count} row${count === 1 ? '' : 's'} carry this tag`}
        >
          {tag}
          <span class="chip-count">{count}</span>
        </button>
      {/each}
      {#if activeTags.size > 0}
        <button type="button" class="chip-clear" onclick={() => (activeTags = new Set())}>
          clear ({activeTags.size})
        </button>
      {/if}
    </div>
  {/if}
  <div class="toolbar-divider" aria-hidden="true"></div>
  <details class="filter-disclosure" open={filtersOpen} ontoggle={toggleFiltersOpen}>
    <summary>
      <span class="dis-label">Filters</span>
      <span class="muted small">price · owned · keep copies · type · ranked-mods threshold</span>
    </summary>
    <div class="filters-panel">
      <div class="filters">
        <label>
          Min avg price
          <input type="number" bind:value={minPrice} min="0" step="1" style="width:60px" />
          <span class="muted">p</span>
        </label>
        <label title="Hides items you own fewer copies of than this. Set to 2 to keep one of each for yourself.">
          Min owned
          <input type="number" bind:value={minOwned} min="1" step="1" style="width:50px" />
        </label>
        <label title="Copies of each item to hold back from selling. Set to 1 to never list your last copy. Unlike Min owned (which only hides rows from this table), this changes what's actually listable.">
          Keep copies
          <input type="number" data-testid="reserve-copies" value={reserveCopies} oninput={setReserveCopies} min="0" step="1" style="width:50px" />
        </label>
        <label>
          Type
          <select bind:value={typeFilter}>
            <option value="all">All</option>
            {#each availableTypes as t}
              <option value={t}>{TYPE_LABELS[t] ?? t}</option>
            {/each}
          </select>
        </label>
        <label title="Hides a row when you have a copy of that mod in `Upgrades` at this rank or higher. 5 ≈ regular maxed (most mods cap at lvl 5). 10 ≈ only Primed/Galvanized maxed. 0 ≈ also hide unranked instances (e.g. rivens). 11 disables the filter.">
          Hide if ranked ≥
          <input type="number" bind:value={hideAtLvl} min="0" max="11" step="1" style="width:55px" />
        </label>
      </div>
    </div>
  </details>
  {#if isDesktop || companionStatus === 'connected'}
    <button
      class="list-cta"
      data-testid="desktop-list"
      onclick={() => openListingFlow()}
      disabled={listableRows.length === 0}
      title="Stage the top rows of the current view (preset + sort) for listing. The in-table name filter and badge chips are not applied — review each row in the modal before sending."
    >List {Math.min(listableRows.length, 50)} on WFM</button>
  {/if}
</section>

{@render pendingBanner()}

{#if results.length > 0 && !scoreExplainerDismissed}
  <div class="score-explainer">
    <strong>About the “Score” column.</strong>
    Expected plat <strong>per day</strong> if you listed everything —
    <code>min(owned, vol_48h / 2) × clearing price</code>, where clearing
    price is the lowest live ask, clamped up to the 90-day median when the
    ask is a lone troll undercut (so one 1p listing can't crater a row).
    Higher = better, uncapped. Items below 2 trades / 48 h get a
    “patience” tag instead of a near-zero score.
    Click <code>?</code> on any column header for the same kind of
    explainer.
    <button class="dismiss" onclick={dismissScoreExplainer} aria-label="Dismiss">×</button>
  </div>
{/if}

{#if results.length > 0}
  <ResultsTable {results} {deltas} {visibleColumns} {presetSort}
    onfiltered={(rows, active) => (tableView = { rows, active })} />
{:else if emptyReason}
  <div class="card empty">
    {#if emptyReason.kind === 'no-market'}
      <div>
        <strong>Nothing in this inventory has live market data.</strong>
        <p class="muted">
          Either nothing here is tradeable, or your market snapshot is
          empty. Check that <code>market.json</code> looks healthy
          ({marketStaleness ?? 'never updated'}).
        </p>
      </div>
    {:else if emptyReason.kind === 'price'}
      <div>
        <strong>{emptyReason.excluded} sellable items are under {minPrice}p average.</strong>
        <p class="muted">Lower the price threshold to see them.</p>
      </div>
      <button onclick={() => relaxFilters({ kind: 'price' })}>Drop min price to 1p</button>
    {:else if emptyReason.kind === 'owned'}
      <div>
        <strong>{emptyReason.excluded} items you own are below the “owned” threshold ({minOwned}).</strong>
        <p class="muted">Most are 1-of-a-kind — set min-owned to 1 to include them.</p>
      </div>
      <button onclick={() => relaxFilters({ kind: 'owned' })}>Set min owned to 1</button>
    {:else if emptyReason.kind === 'type'}
      <div>
        <strong>Nothing in your inventory matches type “{typeFilter}”.</strong>
        <p class="muted">Switch back to All to see everything.</p>
      </div>
      <button onclick={() => relaxFilters({ kind: 'type' })}>Show all types</button>
    {:else if emptyReason.kind === 'kept'}
      <div>
        <strong>All {emptyReason.excluded} candidates have a copy you've ranked to {hideAtLvl}+ in <code>Upgrades</code>.</strong>
        <p class="muted">Raise the threshold (or set 11 to disable) to see them.</p>
      </div>
      <button onclick={() => relaxFilters({ kind: 'kept' })}>Disable the rank filter</button>
    {:else if emptyReason.kind === 'vault'}
      <div>
        <strong>You own no vaulted or vaulting-soon prime parts.</strong>
        <p class="muted">The Vaulted preset only shows parts past (or near) their vault cliff.</p>
      </div>
      <button onclick={() => relaxFilters({ kind: 'vault' })}>Back to Default</button>
    {:else if emptyReason.kind === 'vol' || emptyReason.kind === 'median'}
      <div>
        <strong>Nothing you own clears the Trending liquidity floor.</strong>
        <p class="muted">Trending hides thin-volume rows and penny items so the Δ-sort surfaces real movers.</p>
      </div>
      <button onclick={() => relaxFilters({ kind: emptyReason.kind })}>Back to Default</button>
    {:else if emptyReason.kind === 'ducats'}
      <div>
        <strong>You own no prime parts with a ducat value.</strong>
        <p class="muted">Ducats are Baro Ki'Teer's currency — only prime parts and blueprints carry them.</p>
      </div>
      <button onclick={() => relaxFilters({ kind: 'ducats' })}>Back to Default</button>
    {:else if emptyReason.kind === 'tag' && activePreset === 'sets'}
      <div>
        <strong>You own nothing that trades as part of a prime set.</strong>
        <p class="muted">The Sets preset only shows prime parts and assembled sets.</p>
      </div>
      <button onclick={() => relaxFilters({ kind: 'tag' })}>Back to Default</button>
    {:else if emptyReason.kind === 'tag'}
      <div>
        <strong>Nothing matches the active badge filter{activeTags.size > 1 ? 's' : ''} ({[...activeTags].join(', ')}).</strong>
        <p class="muted">Clear the badge chips (or switch preset) to see the rest.</p>
      </div>
      <button onclick={() => relaxFilters({ kind: 'tag' })}>Back to Default</button>
    {/if}
  </div>
{/if}

<style>
  /* Duplicated from App.svelte's shared styling — Svelte scopes CSS
     per-component, and this codebase's existing extracted components
     already re-declare shared visual classes rather than promoting them
     to global CSS (see DesktopUpdateBanner.svelte). */
  .view-header {
    display: flex;
    align-items: baseline;
    gap: 8px;
    min-height: 28px;
  }
  .view-header h2 {
    font-size: 20px;
    font-weight: 600;
    text-transform: none;
    letter-spacing: -0.01em;
    color: var(--fg);
    margin: 0;
  }
  .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--muted);
    display: inline-block;
  }
  .dot.fresh   { background: var(--good); }
  .dot.aging   { background: var(--warn); }
  .dot.stale   { background: var(--bad); }
  .card {
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 14px 16px;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .card.empty {
    flex-direction: row;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
  }
  .card.empty strong { font-weight: 600; }
  .card.empty p { margin: 4px 0 0 0; }
  .card.empty button { flex-shrink: 0; }
  select {
    font: inherit;
    color: var(--fg);
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 5px 8px;
  }
  code { background: var(--panel-2); padding: 1px 6px; border-radius: 4px; font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: 0.93em; }

  /* Below this point: sell-view-exclusive, moved (not duplicated) from
     App.svelte — nothing else in the app used these selectors. */
  .lede-dot {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 16px;
    height: 16px;
    border-radius: 50%;
    font-size: 11px;
    line-height: 1;
    color: var(--muted);
    border: 1px solid var(--hairline);
    cursor: help;
  }
  .lede-dot:hover, .lede-dot:focus-visible { color: var(--accent); border-color: var(--accent); }

  /* Stats strip — segmented bordered box: one outlined container,
     hairline dividers between cells. */
  .stats {
    display: inline-flex;
    flex-wrap: wrap;
    align-self: flex-start;
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 6px;
    overflow: hidden;
  }
  .stat {
    padding: 7px 16px;
    display: flex;
    align-items: baseline;
    gap: 8px;
    border-right: 1px solid var(--hairline);
  }
  .stat:last-child { border-right: none; }
  .stat .k {
    font-size: 11px;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--muted);
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }
  .stat .v {
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-variant-numeric: tabular-nums;
    font-size: 13px;
    font-weight: 600;
    line-height: 1.3;
  }
  .stat .v.small { font-weight: 500; font-size: 12.5px; }
  .stat .v .unit { font-size: 11px; color: var(--muted); margin-left: 2px; }

  /* Sell toolbar — presets, tag chips, and the Filters disclosure merged
     into one borderless, wrapping strip so the workspace loses a card's
     worth of vertical space and outline on every load. The primary CTA
     anchors the end of this same line via margin-left:auto. */
  .sell-toolbar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 10px 12px;
    padding: 8px 2px;
  }
  .toolbar-group { display: flex; align-items: center; gap: 6px; flex-wrap: wrap; }
  .toolbar-divider { width: 1px; height: 20px; background: var(--hairline); flex: 0 0 auto; }

  /* Preset pills — one-click filter configurations. The active pill is
     accent-bordered AND carries a leading check mark so the selection
     doesn't rely on colour alone. A subtle hint string trails the group. */
  .preset {
    background: transparent;
    border: 1px solid var(--border);
    color: var(--muted);
    border-radius: 5px;
    padding: 4px 12px;
    letter-spacing: 0.02em;
    cursor: pointer;
    transition: color 120ms, border-color 120ms, background 120ms;
    font: inherit;
    font-size: 12px;
  }
  .preset:hover { color: var(--fg); background: var(--panel-2); }
  .preset.active {
    color: var(--accent);
    border-color: var(--accent);
  }
  .preset.active::before { content: '✓ '; }
  .preset-hint { font-size: 11.5px; }

  /* Filters disclosure — an inline toolbar item whose panel floats as a
     popover below the summary instead of pushing the toolbar's other
     groups down. Open state persists in localStorage so power users
     don't re-click every session. */
  .filter-disclosure { position: relative; }
  .filter-disclosure > summary {
    cursor: pointer;
    list-style: none;
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 12px;
    color: var(--muted);
    padding: 4px 10px;
    border: 1px solid var(--border);
    border-radius: 5px;
    user-select: none;
  }
  .filter-disclosure > summary:hover { color: var(--fg); background: var(--panel-2); }
  .filter-disclosure > summary::-webkit-details-marker { display: none; }
  .filter-disclosure > summary::before {
    content: '+';
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 13px;
    color: var(--muted);
    width: 10px;
    display: inline-block;
  }
  .filter-disclosure[open] > summary::before { content: '−'; color: var(--accent); }
  .filter-disclosure[open] > summary { color: var(--accent); border-color: var(--accent); }
  .filter-disclosure > summary .dis-label {
    color: var(--fg);
    font-weight: 600;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    font-size: 11px;
  }
  .filter-disclosure[open] > summary .dis-label { color: var(--accent); }
  .filters-panel {
    position: absolute;
    top: calc(100% + 6px);
    left: 0;
    z-index: 15;
    background: var(--panel-2);
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 12px;
    box-shadow: 0 12px 28px rgba(0, 0, 0, 0.45);
  }
  .filters { display: flex; gap: 14px; align-items: center; flex-wrap: wrap; }
  .filters label {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    font-size: 12px;
    letter-spacing: 0.02em;
    color: var(--muted);
    text-transform: uppercase;
  }
  .filters input, .filters select { text-transform: none; letter-spacing: 0; }

  /* First-session Score explainer. Single dismissable line above the
     table — the casual-flipper persona was confused by what Score meant;
     hover-tooltip alone wasn't enough. localStorage flag means each user
     sees it once. */
  .score-explainer {
    background: var(--panel);
    border: 1px solid var(--border);
    border-left: 3px solid var(--accent);
    border-radius: 8px;
    padding: 10px 38px 10px 14px;
    font-size: 12.5px;
    color: var(--muted);
    line-height: 1.5;
    position: relative;
  }
  .score-explainer strong { color: var(--fg); font-weight: 600; }
  .score-explainer code {
    background: var(--panel-2);
    padding: 1px 6px;
    border-radius: 4px;
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 0.93em;
    color: var(--fg);
  }
  .score-explainer .dismiss {
    position: absolute;
    top: 6px; right: 8px;
    background: transparent;
    border: none;
    color: var(--muted);
    font-size: 16px;
    line-height: 1;
    cursor: pointer;
    padding: 4px 8px;
  }
  .score-explainer .dismiss:hover { color: var(--fg); }

  /* Tag chip row — pills, OR-combined among themselves, AND with the
     filters row above. Inactive chips show the live row-count next to
     the tag so the user can see what's worth toggling. Zero-count chips
     stay visible (strikethrough+muted) so vocabulary is discoverable.
     Chip row caps at ~96px (≈3 wrap rows on desktop, ≈4 on mobile) with
     internal vertical scroll. */
  .tagchips {
    gap: 6px;
    align-items: flex-start;
    max-height: 96px;
    overflow-y: auto;
    align-content: flex-start;
  }
  .chip {
    background: transparent;
    color: var(--muted);
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 4px 10px 4px 12px;
    font-size: 11px;
    letter-spacing: 0.02em;
    cursor: pointer;
    display: inline-flex;
    gap: 6px;
    align-items: center;
    font: inherit;
    line-height: 1.2;
    /* Tap-target — old 21px height failed iOS HIG / WCAG ≥ 24px. 28px
       leaves room without losing the pill aesthetic. */
    min-height: 28px;
    transition: color 120ms ease, border-color 120ms ease, background 120ms ease;
  }
  .chip:hover { color: var(--fg); border-color: var(--accent); }
  .chip.active {
    color: var(--accent);
    border-color: var(--accent);
    background: var(--panel-2);
  }
  .chip.zero {
    text-decoration: line-through;
    opacity: 0.45;
    cursor: default;
  }
  .chip-count {
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 10px;
    color: var(--muted);
  }
  .chip.active .chip-count { color: var(--accent); }
  .chip-clear {
    background: transparent;
    border: none;
    color: var(--muted);
    font-size: 11px;
    cursor: pointer;
    padding: 3px 8px;
  }
  .chip-clear:hover { color: var(--bad); }

  /* Primary CTA inside the presets row — pushed to the far right via
     margin-left:auto so it doesn't visually mix with the chip-style
     presets next to it. Same colour family as the accent. */
  .list-cta {
    margin-left: auto;
    background: var(--accent);
    color: var(--bg);
    border: 1px solid var(--accent);
    font-weight: 600;
  }
  .list-cta:hover:not(:disabled) { filter: brightness(1.1); }
  .list-cta:disabled { opacity: 0.4; cursor: not-allowed; }

  /* Top picks strip. Same carved-panel shell as ResultsTable's .wrap — a
     panel border with a raised (--panel-2) header bar — rather than the
     uniform .card padding, so the picks' one-line "what qualifies" copy
     reads as a fixed header over a list of rows. Rows are hairline-divided. */
  .picks { padding: 0; overflow: hidden; }
  .picks-head {
    display: flex;
    align-items: baseline;
    gap: 10px;
    padding: 12px 16px;
    background: var(--panel-2);
    border-bottom: 1px solid var(--border);
  }
  .picks-title { display: flex; align-items: baseline; gap: 10px; flex-wrap: wrap; }
  .picks-title h3 { margin: 0; font-size: 13px; font-weight: 600; color: var(--fg); }
  .picks-body { display: flex; flex-direction: column; padding: 0 16px; }
  .pick-row {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 10px 0;
    border-top: 1px solid var(--hairline);
  }
  .pick-row:first-of-type { border-top: none; }
  .pick-rank {
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 11px;
    color: var(--muted);
    width: 14px;
    flex-shrink: 0;
    text-align: right;
  }
  /* Single line per pick — the reason sentence truncates by default and
     expands on hover/focus rather than wrapping, so a long sentence can't
     push the row back to two lines. */
  .pick-main { display: flex; flex-direction: row; align-items: baseline; gap: 10px; min-width: 0; flex: 1; }
  .pick-name {
    flex: 0 0 auto;
    max-width: 220px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--fg);
    text-decoration: none;
    font-weight: 600;
    font-size: 13px;
  }
  .pick-name:hover { color: var(--accent); text-decoration: underline; }
  .pick-reason {
    margin: 0;
    flex: 1 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 12.5px;
    color: var(--muted);
    line-height: 1.5;
  }
  .pick-row:hover .pick-reason, .pick-row:focus-within .pick-reason {
    white-space: normal;
    overflow: visible;
  }
  /* Timing tint mirrors .tag.hold/.tag.peak in ResultsTable — same signal,
     same colour, so the strip and the table below read as one vocabulary. */
  .pick-reason.hold { color: var(--warn); }
  .pick-reason.peak { color: var(--good); }
  .pick-vol {
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 11px;
    color: var(--muted);
    margin-left: 2px;
  }
  .pick-score {
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 14px;
    font-weight: 600;
    color: var(--fg);
    flex-shrink: 0;
    text-align: right;
    min-width: 64px;
  }
  .pick-score .unit { font-size: 11px; color: var(--muted); margin-left: 2px; }
  .pick-actions { display: flex; align-items: center; gap: 4px; flex-shrink: 0; }
  .pick-list { font-size: 12px; padding: 5px 12px; min-height: 24px; }
  .pick-snooze {
    background: transparent;
    border: none;
    color: var(--muted);
    font-size: 15px;
    line-height: 1;
    cursor: pointer;
    padding: 4px 7px;
    min-width: 24px;
    min-height: 24px;
  }
  .pick-snooze:hover { color: var(--bad); }
  .picks-all-snoozed { padding: 12px 0; margin: 0; }
</style>
