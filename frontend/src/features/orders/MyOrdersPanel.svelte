<script lang="ts">
  import { onDestroy, untrack, type Snippet } from 'svelte';
  import type { ActivityLog } from '../../ui/activity.svelte';
  import { useDesktopServices } from '../../ui/desktop-context';
  import { createOrdersController, type OrdersInput } from './controller.svelte';
  import Toast from '../../ui/Toast.svelte';
  import { LIQUID_VOL } from '../../domain/sell-priority';
  import { MAX_PLATINUM } from '../../domain/limits';
  let { transport, market = null, sessionEpoch = 0, onauthrequired, ownedQty = null, marketStaleness = null, onsummary, banner, active = true, activity }: OrdersInput & { banner?: Snippet; activity?: ActivityLog } = $props();

  const controller = createOrdersController({ get transport() { return transport; }, get market() { return market; }, get ownedQty() { return ownedQty; }, get sessionEpoch() { return sessionEpoch; }, get onauthrequired() { return onauthrequired; }, get onsummary() { return onsummary; }, get active() { return active; } }, useDesktopServices());
  // Rows on screen but not freshly confirmed (refreshing, or the refresh
  // failed): they stay readable, and changes wait for a confirmed list.
  let unconfirmed = $derived(controller.phase !== 'done');
  // Live checks report in the status strip's activity cell too, so a check
  // started here is still visible from another view.
  let liveActivity: number | null = null;
  $effect(() => {
    const phase = controller.liveTop.phase;
    const { done, total } = controller.liveTop.progress;
    untrack(() => {
      if (!activity) return;
      if (phase === 'running') {
        if (liveActivity == null) liveActivity = activity.begin(`Checking live prices ${done}/${total}`);
        else activity.progress(liveActivity, `Checking live prices ${done}/${total}`);
      } else if (liveActivity != null) {
        const id = liveActivity;
        liveActivity = null;
        if (phase === 'error') activity.finish(id, 'Live check failed', 'bad');
        else activity.finish(id, controller.health.length ? `Live check · ${controller.health.length} to fix` : 'Live check · no issues');
      }
    });
  });
  onDestroy(() => controller.dispose());
  function focusIf(node: HTMLElement, should: boolean): void {
    if (should) node.focus();
  }
</script>

<!-- One table for every listing. Health used to be a second panel that
     listed the same orders with different columns; it is now a column and a
     row state, with each fix beside the item it repairs. Live ask and bid are
     always reserved, so a live check fills cells instead of moving controls. -->
<section class="view-header">
  <h2>My orders</h2>
  <p class="lede">Your active warframe.market listings, fetched live from the desktop app.</p>
  <div class="vh-end">
    {#if controller.phase === 'done' && controller.orders.length > 0}
      <div class="ui-totals" role="group" aria-label="Orders summary">
        <div class="cell"><span class="k">Listed</span><span class="v">{controller.orders.length}{#if controller.listedValue > 0} · {controller.listedValue.toLocaleString()}<span class="unit">p</span>{/if}</span></div>
        {#if controller.counts.issues > 0}<div class="cell attn"><span class="k">Needs fixing</span><span class="v">{controller.counts.issues}</span></div>{/if}
      </div>
    {/if}
    <button
      class="btn primary"
      onclick={controller.checkLive}
      disabled={controller.liveTop.phase === 'running' || controller.phase !== 'done' || controller.orders.length === 0}
      title="Ask warframe.market for the best online asks and bids on each of your sell listings' exact rank / refinement - your own order excluded - and flag what's worth fixing."
    >
      {#if controller.liveTop.phase === 'running'}Checking… {controller.liveTop.progress.done}/{controller.liveTop.progress.total}
      {:else if controller.liveTop.phase === 'done'}Re-check live
      {:else}Check live{/if}
    </button>
  </div>
</section>
{@render banner?.()}

<section class="wrap tw orders" aria-label="My WFM listings">
  <div class="rail">
    <h3>Listings</h3>
    <span class="exp">
      {#if controller.phase === 'loading' || controller.phase === 'idle'}{controller.orders.length > 0 ? 'refreshing…' : 'Fetching orders…'}
      {:else if controller.phase === 'locked'}unlock required
      {:else if controller.phase === 'error'}couldn't load orders
      {:else if controller.queue.length > 0}{controller.counts.issues} of {controller.orders.length} {controller.orders.length === 1 ? 'listing needs' : 'listings need'} attention · fixes apply immediately
      {:else if controller.liveTop.quotes.size > 0}no issues in {controller.orders.length} {controller.orders.length === 1 ? 'listing' : 'listings'} · checked against the live top-of-book
      {:else if ownedQty}no issues in {controller.orders.length} {controller.orders.length === 1 ? 'listing' : 'listings'} · quantities checked against your last scan
      {:else}nothing flagged yet - check live to compare your asks with the online top-of-book
      {/if}
    </span>
    <span class="grow"></span>
    {#if controller.healthSummary.overpriced + controller.healthSummary.underbid > 1}
      <button class="btn" onclick={controller.fixAllPrices} disabled={controller.fixAllBusy || unconfirmed} title="Reprice every flagged listing: match the lowest other ask, or meet the higher bid. Quantity fixes and deletions stay one click each.">Fix all prices</button>
    {/if}
  </div>
  {#if controller.health.length > 0}
    <div class="bar">
      <span class="chips">
        {#if controller.healthSummary.overpriced}<span class="chip warn">{controller.healthSummary.overpriced} above the market</span>{/if}
        {#if controller.healthSummary.underbid}<span class="chip warn">{controller.healthSummary.underbid} under a live bid</span>{/if}
        {#if controller.healthSummary.excessQty}<span class="chip">{controller.healthSummary.excessQty} over-quantity</span>{/if}
        {#if controller.healthSummary.notOwned}<span class="chip bad">{controller.healthSummary.notOwned} not owned</span>{/if}
      </span>
    </div>
  {/if}
  {#if controller.liveTop.phase === 'error' && controller.liveTop.error}
    <div class="line bad">Live check failed: {controller.liveTop.error}</div>
  {/if}
  <div class="bar order-filters">
    <span class="ui-segmented" role="group" aria-label="Show">
      <button type="button" aria-pressed={controller.show === 'all'} onclick={() => (controller.show = 'all')}>All <span class="n">{controller.counts.all}</span></button>
      <button type="button" aria-pressed={controller.show === 'sell'} onclick={() => (controller.show = 'sell')}>Sell <span class="n">{controller.counts.sell}</span></button>
      <button type="button" aria-pressed={controller.show === 'buy'} onclick={() => (controller.show = 'buy')}>Buy <span class="n">{controller.counts.buy}</span></button>
      <button type="button" aria-pressed={controller.show === 'hidden'} onclick={() => (controller.show = 'hidden')}>Hidden <span class="n">{controller.counts.hidden}</span></button>
      <button type="button" class:issues={controller.counts.issues > 0} aria-pressed={controller.show === 'issues'} onclick={() => (controller.show = 'issues')}>Issues <span class="n">{controller.counts.issues}</span></button>
    </span>
    <input class="input order-search" type="text" placeholder="Filter by name…" aria-label="Filter orders by name" bind:value={controller.nameFilter} />
    <span class="grow"></span>
    <button
      class="btn ghost"
      onclick={() => controller.bulkSetVisible(true)}
      disabled={controller.bulkBusy || unconfirmed || controller.orders.every((o) => o.visible == null || o.visible)}
      title="Make every listing visible to buyers"
    >All visible</button>
    <button
      class="btn ghost"
      onclick={() => controller.bulkSetVisible(false)}
      disabled={controller.bulkBusy || unconfirmed || controller.orders.every((o) => o.visible == null || !o.visible)}
      title="Hide every listing from buyers"
    >All hidden</button>
    <button class="btn" onclick={controller.loadOrders} disabled={controller.phase === 'loading'}>Refresh</button>
  </div>

  {#if controller.phase === 'error'}
    <div class="line bad" role="alert">{controller.orders.length > 0 ? `Couldn't refresh orders: ${controller.error}. These are the last confirmed listings; changes are off until a refresh succeeds.` : `Couldn't load orders: ${controller.error}`}</div>
  {/if}
  {#if controller.phase === 'locked'}
    <div class="line"><span class="exp">Unlock warframe.market to see your orders.</span></div>
  {:else if controller.phase === 'done' && controller.orders.length === 0}
    <div class="line"><span class="exp">No active listings.</span></div>
  {:else if controller.orders.length > 0}
    <div class="scroll">
      <table class="tw fixed">
        <colgroup>
          <col />
          <col style="width:6.5rem" />
          <col style="width:4rem" />
          <col style="width:3rem" />
          <col style="width:9rem" />
          <col style="width:5.5rem" />
          <col style="width:5.5rem" />
          <col style="width:15rem" />
          <col style="width:5rem" />
          <col style="width:4.5rem" />
        </colgroup>
        <thead>
          <tr>
            <th class="l">Item</th>
            <th class="l"><span class="sr-only">Fix</span></th>
            <th>Type</th>
            <th>Qty</th>
            <th>Lot price</th>
            <th title="Lowest other online ask for this exact tier (live check)">Live ask</th>
            <th title="Highest online bid for this exact tier (live check)">Live bid</th>
            <th class="l">Health</th>
            <th>Visible</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          {#each controller.shown as o (o.id)}
            {@const busy = controller.busyIds.has(o.id)}
            {@const t = controller.liveTop.quotes.size > 0 ? controller.liveForOrder(o) : null}
            {@const issues = controller.queue.filter((q) => q.id === o.id)}
            <tr class:busy class:attn={issues.length > 0} class:confirming={controller.confirmId === o.id || (controller.healthConfirmId === o.id && issues.length > 0)}>
              <td class="l">{controller.itemName(o)}</td>
              <td class="l act fix">
                {#each issues as q (q.key)}
                  {#if q.kind === 'health'}
                    {#if q.h.kind === 'not-owned' && controller.healthConfirmId === q.id}
                      <button class="btn xs bad" use:focusIf={true} onclick={() => controller.applyFix(q.h)} disabled={busy || unconfirmed} title="Confirm delete">Confirm</button>
                      <button class="btn xs x" onclick={() => controller.cancelDelete(q.id)} title="Cancel" aria-label="Cancel delete">×</button>
                    {:else}
                      <button class="btn xs" class:bad={q.h.kind === 'not-owned'} use:focusIf={controller.restoreFocusTo === q.id} onclick={() => controller.armOrFix(q.h)} disabled={busy || unconfirmed} title={q.h.why}>{controller.healthAction(q.h)}</button>
                    {/if}
                  {:else}
                    <button class="btn xs" onclick={() => controller.reprice(q.d)} disabled={busy || unconfirmed} title="Update this listing to {q.d.suggested}p per unit on warframe.market">Reprice</button>
                  {/if}
                {/each}
              </td>
              <td><span class="type" class:buy={o.side === 'buy'}>{o.side ?? '?'}</span></td>
              <td>{o.quantity ?? '?'}</td>
              <td class="price">
                {#if controller.editingId === o.id}
                  <input type="number" bind:value={controller.editValue} min="1" max={MAX_PLATINUM} aria-label="New price for {controller.itemName(o)}" />
                  <button class="btn xs" onclick={() => controller.saveEdit(o)} disabled={busy || unconfirmed}>save</button>
                  <button class="btn xs x" onclick={() => (controller.editingId = null)} title="Cancel" aria-label="Cancel price edit">×</button>
                {:else}
                  <span class="fg">{o.platinum}<span class="unit">p</span></span>
                  {#if (o.per_trade ?? 1) > 1}<span class="unit"> / {o.per_trade} units</span>{/if}
                  <button class="btn xs ghost edit" onclick={() => controller.startEdit(o)} disabled={busy || unconfirmed} title="Edit price" aria-label="Edit price for {controller.itemName(o)}">✎</button>
                {/if}
              </td>
              <td>{#if t && !t.error && t.low_sell != null}{Number(t.low_sell.toFixed(2))}<span class="unit">p</span>{:else}<span class="muted">-</span>{/if}</td>
              <td>{#if t && !t.error && t.top_buy != null}{Number(t.top_buy.toFixed(2))}<span class="unit">p</span>{:else}<span class="muted">-</span>{/if}</td>
              <td class="l health-cell">
                {#each issues as q (q.key)}
                  {#if q.kind === 'health'}
                    <span class="issue" class:warn={q.h.kind === 'overpriced' || q.h.kind === 'underbid'} class:bad={q.h.kind === 'not-owned'} title={q.h.why}>
                      {#if q.h.kind === 'overpriced' || q.h.kind === 'underbid'}
                        <span class="to">{q.h.current}p → <b>{q.h.suggested}p</b></span>
                        {q.h.kind === 'overpriced' ? 'above the lowest other ask' : 'under a live bid'}
                      {:else if q.h.kind === 'excess-qty'}
                        <span class="to">×{q.h.current} → <b>×{q.h.suggested}</b></span>
                        more listed than owned
                      {:else}
                        <span class="to">×{q.h.current}</span>
                        not in your inventory
                      {/if}
                    </span>
                  {:else}
                    <span class="issue" class:warn={q.d.kind === 'overpriced'} title={controller.driftWhy(q.d)}>
                      <span class="to">{Number(q.d.listed.toFixed(2))}p → <b>{q.d.suggested}p</b></span>
                      {q.d.kind === 'overpriced' ? 'above market' : 'under market'} ({Math.round(q.d.delta_pct * 100)}% {q.d.kind === 'overpriced' ? 'cut' : 'raise'}, snapshot)
                      {#if q.d.thin}<span class="tag thin" title="Below the {LIQUID_VOL}-trade/48h liquidity floor - thin books make this a weak signal.">thin</span>{/if}
                    </span>
                  {/if}
                {:else}<span class="muted">-</span>{/each}
              </td>
              <td>
                <button
                  class="visbtn {o.visible ? 'on' : 'off'}"
                  onclick={() => controller.toggleVisible(o)}
                  disabled={busy || unconfirmed || o.visible == null}
                  title={o.visible == null ? 'Visibility unavailable; refresh orders' : o.visible ? 'Click to make hidden' : 'Click to make visible'}
                  aria-label={o.visible == null ? `?: visibility of ${controller.itemName(o)} unavailable` : o.visible ? `ON: ${controller.itemName(o)} is visible to buyers` : `OFF: ${controller.itemName(o)} is hidden from buyers`}
                ><span class="vis" class:off={!o.visible}>{o.visible == null ? '?' : o.visible ? 'ON' : 'OFF'}</span></button>
              </td>
              <td class="act">
                {#if controller.confirmId === o.id}
                  <button class="btn xs bad" onclick={() => controller.removeOne(o)} disabled={busy || unconfirmed} title="Confirm delete">Confirm</button>
                  <button class="btn xs x" onclick={() => (controller.confirmId = null)} title="Cancel" aria-label="Cancel delete">×</button>
                {:else}
                  <button class="btn xs x" onclick={() => controller.removeOne(o)} disabled={busy || unconfirmed} title="Delete" aria-label="Delete {controller.itemName(o)}">✕</button>
                {/if}
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
    {#if controller.shown.length === 0}
      <div class="line"><span class="exp">No orders match.</span> <button class="btn xs ghost" onclick={() => { controller.show = 'all'; controller.nameFilter = ''; }}>Clear</button></div>
    {/if}
    {#if controller.liveTop.quotes.size === 0 && controller.drifted.length > 0}
      <div class="line">
        <span class="exp">Health compares against the last market snapshot (updated {marketStaleness ?? 'at an unknown time'}) and can't tell whose order is whose&nbsp;- <button class="linkish" onclick={controller.checkLive} disabled={controller.liveTop.phase === 'running'}>check live</button> for exact figures.</span>
      </div>
    {/if}
  {/if}
</section>

<Toast toasts={controller.toasts} ondismiss={controller.dismissToast} />

<style>
  /* Bars may wrap on a narrow desk (the seg + filter + three buttons). */
  .bar { flex-wrap: wrap; row-gap: var(--s1); padding-top: var(--s1); padding-bottom: var(--s1); }
  .chips { display: inline-flex; gap: var(--s1); }
  /* Open issues are a caution to act on; the selected inversion still wins. */
  .ui-segmented > .issues:not([aria-pressed="true"]) { color: var(--warn); }
  .chip {
    display: inline-flex; align-items: center;
    height: var(--ctl-xs);
    padding: 0 var(--s2);
    font-size: var(--text-caption); color: var(--muted);
    border: 1px solid var(--border); border-radius: var(--radius-tag);
  }
  .chip.warn { color: var(--warn); border-color: var(--warn); }
  .chip.bad { color: var(--bad); border-color: var(--bad); }
  .line.bad { color: var(--bad); }
  /* Fixed columns total 58rem; the floor leaves Item at least 14rem before the panel scrolls. */
  .orders table { min-width: 72rem; }
  /* A row with something to fix carries the caution edge; the words in the
     Health cell say what, the edge only repeats it. */
  tr.attn td:first-child { box-shadow: inset 3px 0 0 var(--warn); }
  td.fix { white-space: nowrap; }
  td.fix .btn + .btn { margin-left: var(--s1); }
  td.health-cell { font-family: var(--font-body); font-size: var(--text-caption); white-space: normal; color: var(--muted); }
  td.health-cell .issue { display: block; }
  td.health-cell .issue.warn { color: var(--warn); }
  td.health-cell .issue.bad { color: var(--bad); }
  td.health-cell .to { font-family: var(--font-mono); color: var(--muted); margin-right: var(--s1); }
  td.health-cell .to b { color: var(--fg); font-weight: 600; }
  .sr-only { position: absolute; width: 1px; height: 1px; overflow: hidden; clip: rect(0 0 0 0); white-space: nowrap; }
  /* Price cell: number + a quiet ✎; the inline editor swaps in at 24px. */
  td.price { overflow: visible; }
  td.price .edit { margin-left: var(--s1); color: var(--muted); border-color: transparent; }
  td.price .edit:hover:not(:disabled) { color: var(--fg); border-color: var(--border); }
  td.price input[type="number"] {
    font: inherit; font-family: var(--font-mono); font-size: var(--text-caption);
    height: var(--ctl-xs); width: 4.5rem; padding: 0 var(--s1);
    background: var(--panel-2); color: var(--fg);
    border: 1px solid var(--border); border-radius: var(--radius-input);
    vertical-align: middle;
  }
  /* Visible toggle: a bare button around the ON/OFF pill. */
  .visbtn { appearance: none; background: transparent; border: none; padding: 0; height: var(--ctl-xs); cursor: pointer; }
  .visbtn:hover:not(:disabled) { background: transparent; }
  .visbtn:hover:not(:disabled) .vis { background: var(--panel-2); }
  button.linkish { background: none; border: 0; padding: 0; color: var(--fg); text-decoration: underline dotted; cursor: pointer; font: inherit; }
  .order-filters { gap: var(--s2) var(--s3); padding-block: var(--s2); }
  .order-search { flex: 0 1 15rem; min-width: 10rem; }
  .orders td:last-child .btn { white-space: nowrap; }
</style>
