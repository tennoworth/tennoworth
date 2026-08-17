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
  // $bindable for the same reason (the results table reads it). Everything
  // else here (allPicks/picks/snoozedPicks, pickReason, toggleTag,
  // relaxFilters) is local — nothing outside this view ever touched them.
  import ResultsTable from './ResultsTable.svelte';
  import { PRESETS } from '../lib/presets';
  import { wfmItemUrl, plat } from '../lib/format';
  import { selectPicks, MIN_PICK_SCORE, LIQUID_VOL } from '../lib/sell-priority';

  let {
    minPrice = $bindable(),
    minOwned = $bindable(),
    typeFilter = $bindable(),
    hideAtLvl = $bindable(),
    activeTags = $bindable(),
    tableView = $bindable(),

    resolved, results, deltas, totalPotential,
    prevSummary = null, sinceScan = null, ordersSummary = null,
    marketFreshness, marketStaleness, marketLoadError,
    listableRows, availableTags, availableTypes,
    visibleColumns, presetSort, emptyReason,
    activePreset, reserveCopies, filtersOpen, scoreExplainerDismissed,
    sellOnboardingDismissed, keepCopiesNudgeDismissed,
    isDesktop,

    applyPreset, setReserveCopies, toggleFiltersOpen, dismissScoreExplainer,
    dismissSellOnboarding, dismissKeepCopiesNudge,
    openListingFlow,
    pendingBanner,
  } = $props();

  // Was inline in the template, so it re-filtered the whole results array on
  // EVERY render of this pane — including every keystroke in the name filter,
  // which cannot change the answer. $derived recomputes only when results does.
  let sellableCount = $derived(results.filter((r) => r.sellable > 0).length);

  // Since-last-scan deltas for the summary cells. prevSummary is the previous
  // snapshot pushed through the same filter cascade (App.svelte), so each Δ is
  // in the cell's own units. Null (no previous scan this session) hides them.
  let ownedDelta = $derived(prevSummary ? resolved.owned.size - prevSummary.owned : null);
  let sellableDelta = $derived(prevSummary ? sellableCount - prevSummary.sellable : null);
  let potentialDelta = $derived(prevSummary ? Math.round(totalPotential - prevSummary.potential) : null);
  function fmtDelta(d, unit = '') {
    if (d == null || d === 0) return null;
    const n = Math.abs(d).toLocaleString(undefined, { maximumFractionDigits: 0 });
    return `${d > 0 ? '▲' : '▼'}${n}${unit}`;
  }
  // "Changes only" — session-only view of the rows whose count moved since the
  // last scan (the Δ column's non-zero rows). Not persisted: it is a glance,
  // not a preference, and it means nothing after a reload (no deltas then).
  let changesOnly = $state(false);
  let tableRows = $derived(
    changesOnly ? results.filter((r) => (deltas.get(r.key ?? r.slug) ?? 0) !== 0) : results,
  );

  // Bulk "List on WFM" staging copy. listableRows is already the filtered (or
  // unfiltered) set minus relics (subtyped rows) and no-spare rows; the batch
  // is capped at the first 50 either way.
  let stagedCount = $derived(Math.min(listableRows.length, 50));
  let listLabel = $derived(`List ${stagedCount} on WFM`);
  let listTitle = $derived(
    tableView.active
      ? `Stage the ${stagedCount} rows in your current view (up to the first 50) — the in-table name filter and badge chips are applied; relics and rows with no spare copy are excluded. Review each row in the modal before sending.`
      : `Stage the top ${stagedCount} rows of the current view (up to the first 50) — relics and rows with no spare copy are excluded. Review each row in the modal before sending.`
  );

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
  const PRESET_EMPTY_KINDS = new Set(['tag', 'vault', 'ducats', 'vol', 'median', 'spares']);
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
  <!-- Summary strip: totals with since-last-scan deltas; Listed / Needs fixing
       appear once the orders panel has reported; the last cell is the
       since-scan state with the session-only "Changes only" toggle. -->
  <div class="summary" role="group" aria-label="Sell summary">
    <div class="cell">
      <span class="k">Owned</span>
      <span class="v">{resolved.owned.size.toLocaleString()}</span>
      {#if fmtDelta(ownedDelta)}<span class="d" class:up={ownedDelta > 0} class:down={ownedDelta < 0}>{fmtDelta(ownedDelta)}</span>{/if}
    </div>
    <div class="cell">
      <span class="k">Sellable</span>
      <span class="v">{sellableCount.toLocaleString()}</span>
      {#if fmtDelta(sellableDelta)}<span class="d" class:up={sellableDelta > 0} class:down={sellableDelta < 0}>{fmtDelta(sellableDelta)}</span>{/if}
    </div>
    <div class="cell">
      <span class="k">Potential</span>
      <span class="v">{totalPotential.toLocaleString(undefined, { maximumFractionDigits: 0 })}<span class="unit">p</span></span>
      {#if fmtDelta(potentialDelta)}<span class="d" class:up={potentialDelta > 0} class:down={potentialDelta < 0}>{fmtDelta(potentialDelta, 'p')}</span>{/if}
    </div>
    {#if ordersSummary}
      <div class="cell">
        <span class="k">Listed</span>
        <span class="v">{ordersSummary.live.toLocaleString()}</span>
      </div>
      {#if ordersSummary.issues > 0}
        <div class="cell attn">
          <span class="k">Needs fixing</span>
          <span class="v">{ordersSummary.issues.toLocaleString()}</span>
        </div>
      {/if}
    {/if}
    {#if sinceScan}
      <div class="cell since">
        <span>Since scan: <b>{sinceScan.added}</b> new · <b>{sinceScan.changed}</b> moved · <b>{sinceScan.removed}</b> gone</span>
        <button
          type="button"
          class="changes-toggle"
          class:on={changesOnly}
          aria-pressed={changesOnly}
          onclick={() => (changesOnly = !changesOnly)}
          title="Show only rows whose count changed since the last scan (the Δ column's ▲/▼ rows)."
        >{changesOnly ? '☑' : '☐'} Changes only</button>
      </div>
    {/if}
  </div>
</section>

{#if !sellOnboardingDismissed}
  <div class="card sell-onboarding">
    <div class="so-body">
      <strong>How Sell works</strong>
      <p class="muted">
        Your inventory is ranked by expected plat per day — what to list right
        now. Review the prices, then <strong>List on WFM</strong> posts them
        hidden so no buyer sees them until you flip them visible in
        <strong>My orders</strong>.
      </p>
    </div>
    <button class="dismiss" onclick={dismissSellOnboarding} aria-label="Dismiss">×</button>
  </div>
{/if}

{#if sellOnboardingDismissed && !keepCopiesNudgeDismissed && reserveCopies === 0}
  <div class="card warn-banner keep-nudge">
    <div class="kn-body">
      <strong>Keep copies on by default.</strong>
      <span class="muted">
        Hold back one copy of every item so an underpriced snipe can't strip
        your last copy — set <code>Keep copies</code> to 1 in Filters.
      </span>
    </div>
    <button class="dismiss" onclick={dismissKeepCopiesNudge} aria-label="Dismiss">×</button>
  </div>
{/if}

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
          >Best sells right now, ranked by expected plat/day — patience listings excluded.</span>
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
              {#if p.volume_48h < LIQUID_VOL}
                <span class="pick-tag thin" title="Below the {LIQUID_VOL}-trade/48h liquidity floor — expect to wait for a buyer.">thin</span>
              {/if}
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
        {#if activeTags.size >= 2}
          <p class="tag-or-note">Multiple tags OR together — a row shows if it carries any selected tag.</p>
        {/if}
      {/if}
  <div class="toolbar-divider" aria-hidden="true"></div>
  <details class="filter-disclosure" open={filtersOpen} ontoggle={toggleFiltersOpen}>
    <summary>
      <span class="dis-label">Filters</span>
      <span class="muted small">price · owned · keep copies · type · upgraded mods</span>
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
          Hide upgraded copies at rank ≥
          <input type="number" bind:value={hideAtLvl} min="0" max="11" step="1" style="width:55px" />
        </label>
      </div>
    </div>
  </details>
  {#if isDesktop}
    <button
      class="list-cta"
      data-testid="desktop-list"
      onclick={() => openListingFlow()}
      disabled={listableRows.length === 0}
      title={listTitle}
    >{listLabel}</button>
  {/if}
</section>

{@render pendingBanner()}

{#if results.length > 0}
  <details
    class="score-expander"
    open={!scoreExplainerDismissed}
    ontoggle={(e) => (scoreExplainerDismissed = !e.currentTarget.open)}
  >
    <summary>About the “Score” column</summary>
    <div class="score-details">
      Expected plat <strong>per day</strong> if you listed everything —
      <code>min(owned, vol_48h / 2) × clearing price</code>, where clearing
      price is the lowest live ask, clamped up to the 90-day median when the
      ask is a lone troll undercut (so one 1p listing can't crater a row).
      Higher = better, uncapped. Items below <strong>3 trades / 48 h</strong>
      get a “patience” tag instead of a near-zero score.
      Click <code>?</code> on any column header for the same kind of
      explainer.
    </div>
  </details>
{/if}

{#if results.length > 0}
  <ResultsTable results={tableRows} {deltas} {visibleColumns} {presetSort}
    onfiltered={(rows, active) => (tableView = { rows, active: active || changesOnly })} />
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
    {:else if emptyReason.kind === 'type' && activePreset !== 'spares'}
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
    {:else if emptyReason.kind === 'spares' || (emptyReason.kind === 'type' && activePreset === 'spares')}
      <div>
        <strong>No spare mods or arcanes to sell.</strong>
        <p class="muted">Spares are duplicate copies: every unranked copy when you own a ranked one, otherwise all but one. Nothing you own has a duplicate worth ≥ 3p right now.</p>
      </div>
      <button onclick={() => relaxFilters({ kind: 'spares' })}>Back to Default</button>
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
  /* View header rail: title + the summary strip on one 32px rail. */
  .view-header {
    display: flex;
    align-items: center;
    gap: var(--s2);
    min-height: var(--rail);
    flex-wrap: wrap;
  }
  .view-header h2 {
    font-size: 20px;
    font-weight: 600;
    text-transform: none;
    letter-spacing: -0.01em;
    color: var(--fg);
    margin: 0;
    line-height: 1.5rem;
  }
  /* Summary strip — one outlined container, hairline dividers between cells;
     the since-scan cell rides the right rail. */
  .summary {
    flex: 1 1 auto;
    display: flex;
    align-items: stretch;
    height: var(--rail);
    margin-left: var(--s2);
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: var(--radius-ctl);
    overflow: hidden;
    font-size: 0.75rem;
    white-space: nowrap;
    min-width: 0;
  }
  .summary .cell {
    display: flex;
    align-items: center;
    gap: var(--s2);
    padding: 0 var(--s3);
    border-left: 1px var(--rule) var(--hairline);
  }
  .summary .cell:first-child { border-left: none; }
  .summary .cell .k {
    font-size: 10px;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--muted);
    font-weight: 600;
  }
  .summary .cell .v {
    font-family: var(--font-mono);
    font-variant-numeric: tabular-nums;
    font-size: 13px;
    font-weight: 600;
    line-height: 1rem;
    color: var(--fg);
  }
  .summary .cell .v .unit { font-size: 11px; color: var(--muted); margin-left: 1px; }
  .summary .cell .d {
    font-family: var(--font-mono);
    font-variant-numeric: tabular-nums;
    font-size: 11px;
    line-height: 1rem;
  }
  .summary .cell .d.up { color: var(--good); }
  .summary .cell .d.down { color: var(--bad); }
  .summary .cell.attn .v { color: var(--warn); }
  .summary .cell.since { margin-left: auto; color: var(--muted); overflow: hidden; }
  .summary .cell.since b { color: var(--fg); font-weight: 600; font-family: var(--font-mono); }
  .changes-toggle {
    font: inherit;
    font-size: 11px;
    height: var(--ctl-xs);
    padding: 0 var(--s2);
    color: var(--muted);
    background: transparent;
    border: 1px solid transparent;
    border-radius: var(--radius-ctl);
    cursor: pointer;
    white-space: nowrap;
  }
  .changes-toggle:hover { color: var(--fg); background: var(--panel-2); }
  .changes-toggle.on { color: var(--accent); border-color: var(--accent); background: transparent; }
  .card {
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: var(--radius-panel);
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
    border-radius: var(--radius-input);
    padding: 5px 8px;
  }
  code { background: var(--panel-2); padding: 1px 6px; border-radius: var(--radius-input); font-family: var(--font-mono); font-size: 0.93em; }

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
    border: 1px var(--rule) var(--hairline);
    cursor: help;
  }
  .lede-dot:hover, .lede-dot:focus-visible { color: var(--accent); border-color: var(--accent); }

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
    border-radius: var(--radius-ctl);
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
    border-radius: var(--radius-ctl);
    user-select: none;
  }
  .filter-disclosure > summary:hover { color: var(--fg); background: var(--panel-2); }
  .filter-disclosure > summary::-webkit-details-marker { display: none; }
  .filter-disclosure > summary::before {
    content: '+';
    font-family: var(--font-mono);
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
    border-radius: var(--radius-panel);
    padding: 12px;
    box-shadow: var(--shadow-pop);
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

  /* First-session Score explainer, reworked as an inline <details> expander
     (D9) — one quiet summary line, the how-it's-calculated body folded under
     it. Dismissal = collapsed, persisted via the same flag as before. */
  .score-expander {
    background: var(--panel);
    border: 1px solid var(--border);
    border-left: 3px solid var(--accent);
    border-radius: var(--radius-panel);
    padding: 0 14px;
    position: relative;
  }
  .score-expander > summary {
    cursor: pointer;
    list-style: none;
    font-size: 12.5px;
    font-weight: 600;
    color: var(--fg);
    line-height: 1.5;
    padding: 10px 0;
    display: flex;
    align-items: center;
    gap: 8px;
    user-select: none;
  }
  .score-expander > summary::-webkit-details-marker { display: none; }
  .score-expander > summary::before {
    content: '+';
    font-family: var(--font-mono);
    color: var(--muted);
    width: 10px;
    display: inline-block;
  }
  .score-expander[open] > summary::before { content: '−'; color: var(--accent); }
  .score-expander > summary:hover { color: var(--accent); }
  .score-details {
    font-size: 12.5px;
    color: var(--muted);
    line-height: 1.5;
    padding: 0 0 10px;
  }
  .score-details strong { color: var(--fg); font-weight: 600; }
  .score-details code {
    background: var(--panel-2);
    padding: 1px 6px;
    border-radius: var(--radius-input);
    font-family: var(--font-mono);
    font-size: 0.93em;
    color: var(--fg);
  }

  /* First-session sell onboarding — above the stats strip, dismissed once. */
  .sell-onboarding {
    flex-direction: row;
    align-items: flex-start;
    justify-content: space-between;
    gap: 12px;
    border-left: 3px solid var(--accent);
  }
  .sell-onboarding .so-body { min-width: 0; }
  .sell-onboarding strong { color: var(--fg); font-weight: 600; font-size: 13px; }
  .sell-onboarding p { margin: 4px 0 0; font-size: 12.5px; line-height: 1.5; }
  .sell-onboarding .dismiss {
    background: transparent;
    border: none;
    color: var(--muted);
    font-size: 16px;
    line-height: 1;
    cursor: pointer;
    padding: 3px;
    flex-shrink: 0;
  }
  .sell-onboarding .dismiss:hover { color: var(--fg); }

  /* Keep-copies nudge — one-time education after the onboarding card is
     dismissed; reappearing until dismissed is the intent (it's a habit, not
     a task). */
  .keep-nudge {
    flex-direction: row;
    align-items: flex-start;
    justify-content: space-between;
    gap: 12px;
    border-left: 3px solid var(--warn);
  }
  .keep-nudge .kn-body { min-width: 0; }
  .keep-nudge strong { color: var(--fg); font-weight: 600; font-size: 13px; }
  .keep-nudge .muted { font-size: 12.5px; line-height: 1.5; }
  .keep-nudge code { font-family: var(--font-mono); font-size: 0.93em; }
  .keep-nudge .dismiss {
    background: transparent;
    border: none;
    color: var(--muted);
    font-size: 16px;
    line-height: 1;
    cursor: pointer;
    padding: 3px;
    flex-shrink: 0;
  }
  .keep-nudge .dismiss:hover { color: var(--fg); }

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
    border-radius: var(--radius-input);
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
    font-family: var(--font-mono);
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
    color: var(--on-accent);
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
    border-top: 1px var(--rule) var(--hairline);
  }
  .pick-row:first-of-type { border-top: none; }
  .pick-rank {
    font-family: var(--font-mono);
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
  /* Thin-volume tag — a sibling of .pick-reason inside .pick-main so it stays
     OUTSIDE the truncating <p> and is visible at a glance. */
  .pick-tag {
    flex-shrink: 0;
    padding: 1px 6px;
    font-size: 9.5px;
    font-weight: 600;
    letter-spacing: 0.1em;
    text-transform: uppercase;
    border: 1px solid currentColor;
    border-radius: var(--radius-tag);
    color: var(--warn);
  }
  .tag-or-note {
    margin: 0;
    flex-basis: 100%;
    font-size: 11.5px;
    color: var(--muted);
  }
  .pick-vol {
    font-family: var(--font-mono);
    font-size: 11px;
    color: var(--muted);
    margin-left: 2px;
  }
  .pick-score {
    font-family: var(--font-mono);
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
