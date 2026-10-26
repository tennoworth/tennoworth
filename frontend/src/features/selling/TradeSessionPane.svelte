<script lang="ts">
  import { useDesktopServices } from '../../ui/desktop-context';
  const { desktopTradeSessionState, desktopLiveTopPrices, listenForTauriEvent, evaluateTradeSession } = useDesktopServices();
  import { onMount, onDestroy, untrack, type Snippet } from 'svelte';
  import BuyerAlternatives from './BuyerAlternatives.svelte';
  import { LiveTopController } from './live-top.svelte';
  const liveTop = new LiveTopController({ desktopLiveTopPrices, listenForTauriEvent });
  onDestroy(() => liveTop.dispose());
  import { computeResults } from '../../domain/filter-engine';
  import { SESSION_MODES, type SessionPlan, type SessionMode, type SessionRow, type SessionCandidate } from '../../domain/trade-session';
  
import { ALLOWANCE_CHANGED_EVENT } from '../../contracts/events';
  
  import { humanError } from '../../contracts/errors';
  import { MAX_PLAN_ITEMS } from '../../domain/limits';
  import { cachedUnitMarket } from '../../domain/order-prices';
  import type { Market, MarketItemEntry, OwnedRecord, TradeSessionState } from '../../contracts/data';
  import type { ScoredInventoryFact } from '../../contracts/generated/domain';
  import type { Verdict } from '../../contracts/generated/domain';

  let { owned, market, reserveCopies, advice, scanning, onscan, onreview, nativeFacts = new Map(), availability, listingBlockReason = null, onrecheck, listingActionLabel = 'Check WFM listings', keep, active = true, sessionEpoch = 0 }: {
    keep?: Snippet;
    /** Bumped when the WFM session changes; live prices belong to one account. */
    sessionEpoch?: number;
    /** Whether the view is on screen; it stays mounted while hidden. */
    active?: boolean;
    owned: Map<string, OwnedRecord>; market: Market | null; reserveCopies: number;
    nativeFacts?: Map<string, ScoredInventoryFact>;
    advice: Map<string, Verdict>; scanning: boolean; onscan: () => Promise<void>;
    onreview: (rows: SessionRow[], budget: number, state: TradeSessionState) => void;
    availability?: ReadonlyMap<string, number>;
    listingBlockReason?: string | null;
    onrecheck?: () => void;
    listingActionLabel?: string;
  } = $props();
  let mode = $state<SessionMode>('fast');
  let budget = $state<number | undefined>();
  let target = $state<number | undefined>();
  let sessionData = $state<TradeSessionState | null>(null);
  let error = $state<string | null>(null);
  let loading = $state(true);
  let liveBusy = $derived(liveTop.phase === 'running');
  let priceNote = $state<string | null>(null);
  let livePrices = $state<Record<string, Partial<MarketItemEntry>>>({});
  let priceSources = $state<Record<string, string>>({});
  let buyerSelection = $state<SessionRow | null>(null);
  let disposed = false;
  let request = 0;
  let budgetInitialized = false;
  let lastPositiveBudget = 8;
  let includeSets = $state(true);
  const filters = { minPrice: 0, minOwned: 0, typeFilter: 'all', hideAtLvl: Infinity,
    activeTags: new Set<string>(), vaultOnly: false, ducatsOnly: false, minVol: 0,
    minMedian: 0, typesAny: [], sparesOnly: false, adviceOnly: false };
  let parts = $derived(computeResults(owned, market, filters, reserveCopies, advice, availability, nativeFacts).filter(r => !r.slug.endsWith('_set')).map(r => ({
    key: r.key, slug: r.slug, name: r.name, owned: r.owned, leveled: r.leveled,
    sellable: Math.min(r.sellable, sessionData?.quantities[r.slug] ?? 0), subtype: r.subtype,
    type: r.type, hold: r.timing === 'hold' || r.advice === 'hold',
    bulk: sessionData?.bulk_slugs.includes(r.slug) ?? false,
    supported: sessionData?.supported_slugs == null ? undefined : sessionData.supported_slugs.includes(r.slug),
    market: { ...cachedUnitMarket(market!.items[r.slug], sessionData?.bulk_slugs.includes(r.slug)), ...livePrices[r.slug] },
  })));
  let candidates = $derived.by<SessionCandidate[]>(() => {
    if (!includeSets || !market || !sessionData) return parts;
    const sets: SessionCandidate[] = [];
    for (const [slug, components] of Object.entries(sessionData.set_recipes ?? {})) {
      const recipe = Object.entries(components);
      const rows = recipe.map(([part]) => parts.filter(row => row.slug === part && !row.subtype));
      if (!recipe.length || rows.some(rows => rows.length !== 1) || !market.items[slug]) continue;
      const sellable = Math.min(...recipe.map(([, count], i) => Math.floor(rows[i][0].sellable / count)));
      if (sellable < 1) continue;
      sets.push({ key: slug, slug, name: `${market.set_to_parts?.[slug]?.name ?? slug} Set`,
        owned: Math.min(...recipe.map(([, count], i) => Math.floor(rows[i][0].owned / count))),
        sellable, leveled: 0, subtype: null, type: 'Set', hold: advice.get(slug)?.advice === 'hold',
        bulk: false, supported: true, components, market: { ...market.items[slug], ...livePrices[slug] } });
    }
    return [...parts, ...sets];
  });
  let plan = $state<SessionPlan>({ rows: [], trades: 0, total: 0, excluded: [], target: null, shortfall: null });
  let planning = $state(false);
  let planningError = $state<string | null>(null);
  $effect(() => {
    // Until the session loads every quantity is 0, so there is nothing to plan.
    if (!sessionData) return;
    const input = { candidates, mode, budget: budget ?? 0, target: target ?? null };
    let cancelled = false;
    planning = true;
    planningError = null;
    void evaluateTradeSession(input).then(next => {
      if (!cancelled) plan = next;
    }).catch(e => {
      if (!cancelled) planningError = humanError(e);
    }).finally(() => {
      if (!cancelled) planning = false;
    });
    return () => { cancelled = true; };
  });
  let cap = $derived(Math.min(MAX_PLAN_ITEMS, sessionData?.allowance.remaining ?? 0));
  let retained = $state<SessionPlan | null>(null);
  $effect(() => { if (plan.rows.length > 0) retained = plan; });
  let display = $derived(cap === 0 && retained ? retained : plan);
  let allowance = $derived(sessionData?.allowance);

  function partsAsk(components: Record<string, number>): number | null {
    let total = 0;
    for (const [slug, count] of Object.entries(components)) {
      const ask = parts.find(part => part.slug === slug)?.market.low_sell;
      if (ask == null || !Number.isFinite(ask) || ask <= 0) return null;
      total += ask * count;
    }
    return total;
  }

  async function refresh() {
    const current = ++request;
    try {
      const next = await desktopTradeSessionState();
      if (disposed || current !== request) return;
      if (!next?.allowance || !next.quantities || !Array.isArray(next.bulk_slugs)) throw new Error('Trade Session data is unavailable. Try scanning again.');
      if (sessionData && sessionData.allowance.snapshot_id !== next.allowance.snapshot_id) retained = null;
      sessionData = next;
      const maximum = Math.min(MAX_PLAN_ITEMS, next.allowance.remaining ?? 0);
      if (!budgetInitialized && maximum > 0) {
        budget = Math.min(8, maximum);
        budgetInitialized = true;
      } else if (budget != null) {
        budget = Math.min(budget === 0 ? lastPositiveBudget : budget, maximum);
      }
      if (budget != null && budget > 0) lastPositiveBudget = budget;
      error = null;
    } catch (e) {
      if (!disposed && current === request) error = humanError(e);
    } finally { if (!disposed && current === request) loading = false; }
  }

  async function scan() { await onscan(); await refresh(); }

  async function refreshPrices() {
    priceNote = null;
    const rows = plan.rows;
    const quotes = await liveTop.check(rows.map(r => ({ slug: r.slug, rank: 0, subtype: null })));
    if (disposed) return;
    if (!quotes) { priceNote = liveTop.error; return; }
    let failed = 0;
    const next = { ...livePrices };
    const sources = { ...priceSources };
    for (const row of rows) {
      const q = quotes.find(q => q.slug === row.slug && (q.rank ?? 0) === 0 && q.subtype == null);
      if (!q || q.error) { failed++; continue; }
      next[q.slug] = { low_sell: q.low_sell ?? 0, top_buy: q.top_buy ?? 0, price_basis: 'unit' };
      sources[q.slug] = `${q.low_sell == null ? 'No live ask; cached reference' : 'Live checked'} · ${new Date().toLocaleTimeString()}`;
    }
    livePrices = next;
    priceSources = sources;
    priceNote = failed ? `${failed} price checks failed; those items retain their previous prices.` : 'Online competitors checked; your own orders are excluded.';
  }

  // Live prices were fetched for the previous account (its platform, its own
  // orders excluded), so a new session starts from the cached snapshot again.
  let pricesEpoch = untrack(() => sessionEpoch);
  $effect(() => {
    const epoch = sessionEpoch;
    untrack(() => {
      if (epoch === pricesEpoch) return;
      pricesEpoch = epoch;
      liveTop.reset();
      livePrices = {};
      priceSources = {};
      priceNote = null;
    });
  });

  // Back on screen after being hidden: refresh at once rather than at the next poll.
  let wasActive = untrack(() => active);
  $effect(() => {
    const onScreen = active;
    untrack(() => { if (onScreen && !wasActive) void refresh(); wasActive = onScreen; });
  });

  onMount(() => {
    void refresh();
    const stop = listenForTauriEvent(ALLOWANCE_CHANGED_EVENT, () => void refresh());
    const timer = setInterval(() => { if (active) void refresh(); }, 10_000);
    return () => { disposed = true; clearInterval(timer); stop(); };
  });
</script>

<section class="view-header">
  <h2>Trade Session</h2>
  <p class="lede">Choose what matters, review a batch, return to Warframe.</p>
</section>
{@render keep?.()}

<div class="ui-stack">
  <section class="ui-panel session-setup" aria-label="Trade allowance and budget">
    <div class="ui-toolbar planning-controls">
      <div class="ui-field">
        <span>Trade goal</span>
        <div class="ui-segmented modes" role="group" aria-label="Trade intent">
          {#each SESSION_MODES as choice}
            <button aria-pressed={mode === choice.id} title={choice.description}
              disabled={cap === 0 && !!retained} onclick={() => mode = choice.id}>{choice.name}</button>
          {/each}
        </div>
      </div>
      <label class="ui-field budget-field">Trade budget
        <input class="ui-input" aria-label="Trade budget" type="number" min="1" max={cap} step="1"
          bind:value={budget} disabled={cap === 0} />
      </label>
      <span class="muted">{cap > 0 && cap === allowance?.remaining ? `Can't exceed the ${cap} trades left today.` : `Up to ${cap} estimated trades per batch.`}</span>
    </div>
    <p class="muted">{SESSION_MODES.find(choice => choice.id === mode)?.description}</p>
    <details class="session-options">
      <summary>Target and complete sets</summary>
      <div class="ui-toolbar">
        <label class="ui-field">Platinum target · optional
          <input class="ui-input" aria-label="Platinum target" type="number" min="1" step="1" bind:value={target} placeholder="No target" disabled={cap === 0 && !!retained} />
        </label>
        <label class="sets-check"><input type="checkbox" bind:checked={includeSets} disabled={cap === 0 && !!retained} /> Include complete owned sets</label>
      </div>
    </details>
    <div class="ui-toolbar allowance-status">
      <strong>
        {#if loading}Reading trade allowance…
        {:else if allowance?.remaining != null}
          {allowance.confidence === 'estimated' ? 'About ' : ''}{allowance.remaining} trades left today
          {#if allowance.mastery_rank != null} · MR {allowance.mastery_rank}{/if}
        {:else}Trade allowance unknown{/if}
      </strong>
      <button class="btn" onclick={scan} disabled={scanning}>{scanning ? 'Scanning…' : 'Scan game'}</button>
      {#if error}<button class="btn ghost" onclick={refresh}>Retry</button>{/if}
    </div>
    {#if allowance?.observed_at}
      <p class="muted">Last scan {new Date(allowance.observed_at * 1000).toLocaleString()} · {allowance.confidence}
        {#if allowance.monitoring} · local trade tracking active{/if}</p>
    {/if}
    {#if allowance?.reason}<p class="muted">{allowance.reason}</p>{/if}
    {#if error}<p class="ui-notice" data-tone="bad" role="alert">{error}</p>{/if}
    {#if !loading && allowance?.remaining === 0}
      <p class="ui-notice" data-tone="warn">No trades remaining. This batch is read-only; existing WFM listings are unchanged. The allowance resets at 00:00 UTC.</p>
    {/if}
  </section>

  <section class="wrap tw session-batch" aria-label="Suggested batch">
    <div class="rail"><h3>Suggested batch</h3><span class="exp">Review before posting</span></div>
    <div class="ui-stack session-body">
    {#if listingBlockReason}<p class="ui-notice" data-tone="warn" role="status">{listingBlockReason} {#if onrecheck}<button class="btn" onclick={onrecheck}>{listingActionLabel}</button>{/if}</p>{/if}
    <div class="ui-toolbar">
      <h3>{display.rows.length} listings · {display.trades} estimated trades · {display.total.toLocaleString()}p potential</h3>
      <button class="btn ghost" onclick={refreshPrices} disabled={planning || !!planningError || liveBusy || !plan.rows.length || cap === 0}>
        {liveBusy ? 'Checking prices…' : 'Check live prices'}
      </button>
      <button class="btn primary" disabled={!!listingBlockReason || loading || planning || !!planningError || !!error || cap === 0 || !plan.rows.length || (budget ?? 0) > cap || liveBusy}
        onclick={() => !listingBlockReason && sessionData && onreview(plan.rows, budget ?? 0, sessionData)}>Review batch</button>
    </div>
    <p class="muted">Potential assumes sales at the listed unit asks, rounded up to whole platinum. Bids are per-unit comparisons, not instant proceeds.
      Cached market: {market?.updated_at ? new Date(market.updated_at).toLocaleString() : 'unknown age'}.
      </p>
    {#if planning}<p role="status">Updating suggested batch…</p>{/if}
    {#if planningError}<p class="ui-notice" data-tone="bad" role="alert">Could not update the suggested batch: {planningError} Change the intent or budget to retry.</p>{/if}
    {#if priceNote}<p role="status">{priceNote}</p>{/if}
    {#if display.target != null}
      <p class="ui-notice" data-tone={display.shortfall === 0 ? 'good' : 'warn'}>
        {display.shortfall === 0 ? 'Target covered at listing prices, if these items sell.' : `${display.shortfall?.toLocaleString()}p short of the target within this budget.`}
      </p>
    {/if}
    {#if display.rows.length}
      <!-- svelte-ignore a11y_no_noninteractive_tabindex (Keyboard focus is needed to scroll the wide table.) -->
      <div class="wrap tw" tabindex="0" role="region" aria-label="Suggested listings; scroll for all columns">
        <table class="tw fixed session-table">
          <colgroup><col style="width:22%" /><col style="width:7%" /><col style="width:9%" /><col style="width:9%" /><col style="width:12%" /><col style="width:7%" /><col style="width:9%" /><col style="width:25%" /></colgroup>
          <thead><tr><th class="l">Item</th><th>Quantity</th><th>Units / trade</th><th>Est. trades</th><th>Unit ask</th><th>Unit bid</th><th>Potential</th><th class="l">Why this item</th></tr></thead>
          <tbody>
            {#each display.rows as row (row.key)}
              <tr>
                <td class="l">{row.name}<small>{row.sellable} confirmed sellable / {row.owned} owned · rank 0 where applicable</small>
                  {#if row.components}
                    {@const reference = partsAsk(row.components)}
                    <small>Per set: {Object.entries(row.components).map(([slug, count]) => `${parts.find(part => part.slug === slug)?.name ?? slug} ×${count}`).join(', ')}</small>
                    <small>Parts reference: {reference == null ? 'Unknown' : `${Number(reference.toFixed(2))}p`} · set ask {row.platinum}p</small>
                  {/if}
                  <button class="btn ghost xs" onclick={() => buyerSelection = { ...row }}>Compare buyers</button>
                </td>
                <td>{row.quantity}</td><td>{row.per_trade}</td><td>{row.trades}</td><td>{row.platinum}p<small>{priceSources[row.slug] ?? 'Cached reference'}</small></td>
                <td>{row.bid == null ? 'Unknown' : `${Number(row.bid.toFixed(2))}p`}</td><td>{row.quantity * row.platinum}p</td><td class="reason">{row.reason}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
      <p class="muted">Estimated trades = quantity ÷ suggested units per trade, summed across the batch. Buyers may agree different quantities. Posting listings does not spend in-game trades.</p>
    {:else if !loading && !planning && !planningError}
      <p>No eligible batch yet. Scan the game, check protected quantities, or choose another intent.</p>
    {/if}
    {#if plan.excluded.length}
      <details><summary>{plan.excluded.length} items not included in this mode</summary>
        <ul>{#each plan.excluded as row}<li>{row.name}: {row.reason}</li>{/each}</ul>
      </details>
    {/if}
    </div>
  </section>
  {#if buyerSelection}
    {#key buyerSelection}
      <BuyerAlternatives slug={buyerSelection.slug} name={buyerSelection.name} quantity={buyerSelection.quantity}
        ask={buyerSelection.platinum} onclose={() => buyerSelection = null} />
    {/key}
  {/if}
</div>

<style>
  .planning-controls { align-items: end; gap: var(--s3) var(--s4); }
  .session-setup { gap: var(--s2); }
  .budget-field input { width: 5rem; }
  .session-options { padding-block: var(--s2); border-block: 1px var(--rule) var(--hairline); }
  .session-options > .ui-toolbar { margin-top: var(--s2); gap: var(--s4); align-items: end; }
  .sets-check { display: flex; align-items: center; gap: var(--s2); min-height: var(--ctl-lg); }
  .allowance-status { font-size: var(--text-control); }
  .session-body { padding: var(--s3) var(--inset); }
  .session-table { min-width: 70rem; }
  .reason { min-width: 18rem; white-space: normal; font-family: var(--font-body); }
  small { display: block; font-family: var(--font-body); font-size: var(--text-caption); color: var(--muted); line-height: var(--leading-body); }
  h3 { font-size: var(--text-section); margin: 0; }
  p { margin: 0; }
  .ui-input { max-width: 12rem; }
</style>
