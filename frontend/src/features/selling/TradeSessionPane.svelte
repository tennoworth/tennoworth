<script lang="ts">
  import { useDesktopServices } from '../../ui/desktop-context';
  const { desktopTradeSessionState, desktopLiveTopPrices, listenForTauriEvent } = useDesktopServices();
  import { onMount } from 'svelte';
  import { computeResults } from '../../domain/filter-engine';
  import { SESSION_MODES, selectSession, type SessionMode, type SessionRow } from '../../domain/trade-session';
  
import { ALLOWANCE_CHANGED_EVENT } from '../../contracts/events';
  
  import { humanError } from '../../contracts/errors';
  import { MAX_PLAN_ITEMS } from '../../domain/limits';
  import { cachedUnitMarket } from '../../domain/order-prices';
  import type { Market, MarketItemEntry, OwnedRecord, TradeSessionState } from '../../contracts/data';
  import type { Verdict } from '../../domain/advisor';

  let { owned, market, reserveCopies, advice, scanning, onscan, onreview }: {
    owned: Map<string, OwnedRecord>; market: Market | null; reserveCopies: number;
    advice: Map<string, Verdict>; scanning: boolean; onscan: () => Promise<void>;
    onreview: (rows: SessionRow[], budget: number, state: TradeSessionState) => void;
  } = $props();
  let mode = $state<SessionMode>('fast');
  let budget = $state<number | undefined>();
  let target = $state<number | undefined>();
  let sessionData = $state<TradeSessionState | null>(null);
  let error = $state<string | null>(null);
  let loading = $state(true);
  let liveBusy = $state(false);
  let priceNote = $state<string | null>(null);
  let livePrices = $state<Record<string, Partial<MarketItemEntry>>>({});
  let priceSources = $state<Record<string, string>>({});
  let disposed = false;
  let request = 0;
  let budgetInitialized = false;
  let lastPositiveBudget = 8;
  const filters = { minPrice: 0, minOwned: 0, typeFilter: 'all', hideAtLvl: Infinity,
    activeTags: new Set<string>(), vaultOnly: false, ducatsOnly: false, minVol: 0,
    minMedian: 0, typesAny: [], sparesOnly: false, adviceOnly: false };
  let candidates = $derived(computeResults(owned, market, filters, reserveCopies, advice).map(r => ({
    key: r.key, slug: r.slug, name: r.name, owned: r.owned, leveled: r.leveled,
    sellable: Math.min(r.sellable, sessionData?.quantities[r.slug] ?? 0), subtype: r.subtype,
    type: r.type, hold: r.timing === 'hold' || r.advice === 'hold',
    bulk: sessionData?.bulk_slugs.includes(r.slug) ?? false,
    supported: sessionData?.supported_slugs == null ? undefined : sessionData.supported_slugs.includes(r.slug),
    market: { ...cachedUnitMarket(market!.items[r.slug], sessionData?.bulk_slugs.includes(r.slug)), ...livePrices[r.slug] },
  })));
  let plan = $derived(selectSession(candidates, mode, budget ?? 0, target));
  let cap = $derived(Math.min(MAX_PLAN_ITEMS, sessionData?.allowance.remaining ?? 0));
  let retained = $state<ReturnType<typeof selectSession> | null>(null);
  $effect(() => { if (plan.rows.length > 0) retained = plan; });
  let display = $derived(cap === 0 && retained ? retained : plan);
  let allowance = $derived(sessionData?.allowance);

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
    liveBusy = true;
    priceNote = null;
    const rows = plan.rows;
    try {
      const quotes = await desktopLiveTopPrices(rows.map(r => ({ slug: r.slug, rank: 0, subtype: null })));
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
    } catch (e) { priceNote = humanError(e); }
    finally { liveBusy = false; }
  }

  onMount(() => {
    void refresh();
    const stop = listenForTauriEvent(ALLOWANCE_CHANGED_EVENT, () => void refresh());
    const timer = setInterval(() => void refresh(), 10_000);
    return () => { disposed = true; clearInterval(timer); stop(); };
  });
</script>

<section class="view-header">
  <h2>Trade Session</h2>
  <p class="lede">Choose what matters, review a batch, return to Warframe.</p>
</section>

<div class="ui-stack">
  <div class="ui-toolbar modes" role="group" aria-label="Trade intent">
    {#each SESSION_MODES as choice}
      <button class="btn mode" class:primary={mode === choice.id} aria-pressed={mode === choice.id}
        disabled={cap === 0 && !!retained}
        onclick={() => mode = choice.id}>
        <span>{choice.name}</span><span class="description">{choice.description}</span>
      </button>
    {/each}
  </div>

  <section class="ui-panel ui-stack" aria-label="Trade allowance and budget">
    <div class="ui-toolbar">
      <strong>
        {#if loading}Reading trade allowance…
        {:else if allowance?.remaining != null}
          {allowance.confidence === 'estimated' ? 'About ' : ''}{allowance.remaining} remaining
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
    <div class="ui-toolbar">
      <label class="ui-field">Trade budget
        <input class="ui-input" aria-label="Trade budget" type="number" min="1" max={cap} step="1"
          bind:value={budget} disabled={cap === 0} />
      </label>
      <label class="ui-field">Platinum target · optional
        <input class="ui-input" aria-label="Platinum target" type="number" min="1" step="1" bind:value={target} placeholder="No target" disabled={cap === 0 && !!retained} />
      </label>
      <span class="muted">Up to {cap} estimated trades.</span>
    </div>
  </section>

  <section class="ui-panel ui-stack" aria-label="Suggested batch">
    <div class="ui-toolbar">
      <h3>{display.rows.length} listings · {display.trades} estimated trades · {display.total.toLocaleString()}p potential</h3>
      <button class="btn primary" disabled={loading || !!error || cap === 0 || !plan.rows.length || (budget ?? 0) > cap || liveBusy}
        onclick={() => sessionData && onreview(plan.rows, budget ?? 0, sessionData)}>Review batch</button>
      <button class="btn ghost" onclick={refreshPrices} disabled={liveBusy || !plan.rows.length || cap === 0}>
        {liveBusy ? 'Checking prices…' : 'Check live prices'}
      </button>
    </div>
    <p class="muted">Potential assumes sales at the listed unit asks, rounded up to whole platinum. Bids are per-unit comparisons, not instant proceeds.
      Cached market: {market?.updated_at ? new Date(market.updated_at).toLocaleString() : 'unknown age'}.
      </p>
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
                <td class="l">{row.name}<small>{row.sellable} confirmed sellable / {row.owned} owned · rank 0 where applicable</small></td>
                <td>{row.quantity}</td><td>{row.per_trade}</td><td>{row.trades}</td><td>{row.platinum}p<small>{priceSources[row.slug] ?? 'Cached reference'}</small></td>
                <td>{row.bid == null ? 'Unknown' : `${Number(row.bid.toFixed(2))}p`}</td><td>{row.quantity * row.platinum}p</td><td class="reason">{row.reason}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
      <p class="muted">Estimated trades = quantity ÷ suggested units per trade, summed across the batch. Buyers may agree different quantities. Posting listings does not spend in-game trades.</p>
    {:else if !loading}
      <p>No eligible batch yet. Scan the game, check protected quantities, or choose another intent.</p>
    {/if}
    {#if plan.excluded.length}
      <details><summary>{plan.excluded.length} items not included in this mode</summary>
        <ul>{#each plan.excluded as row}<li>{row.name}: {row.reason}</li>{/each}</ul>
      </details>
    {/if}
  </section>
</div>

<style>
  .modes { align-items: stretch; }
  .mode { flex: 1 1 12rem; min-height: var(--ctl-lg); height: auto; display: flex; flex-direction: column; align-items: flex-start; text-align: left; white-space: normal; padding: var(--s3); }
  .description { font-family: var(--font-body); font-size: var(--text-caption); line-height: var(--leading-body); text-transform: none; letter-spacing: normal; font-weight: 400; }
  .session-table { min-width: 70rem; }
  .reason { min-width: 18rem; white-space: normal; font-family: var(--font-body); }
  small { display: block; font-size: var(--text-caption); color: var(--muted); line-height: var(--leading-body); }
  h3 { font-size: var(--text-section); margin: 0; }
  p { margin: 0; }
  .ui-input { max-width: 12rem; }
</style>
