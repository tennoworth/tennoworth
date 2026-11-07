<script lang="ts">
  import { fitTable } from '../../ui/table-fit';
  import { searchShortcut } from '../../ui/keyboard.svelte';
  import { busy as waiting, focusLater, roving } from '../../ui/keyboard-actions';
  import { onDestroy, untrack, type Snippet } from 'svelte';
  import type { ActivityLog } from '../shell/activity.svelte';
  import { useDesktopServices } from '../shell/desktop-context';
  import { createOrdersController, type OrdersInput } from './controller.svelte';
  import Toast from '../../ui/Toast.svelte';
  import ItemArt from '../../ui/ItemArt.svelte';
  import { LIQUID_VOL } from '../../domain/sell-priority';
  import { MAX_PLATINUM } from '../../domain/limits';
  import { formatLocale, t } from '../../i18n/i18n.svelte';
  let { transport, market = null, sessionEpoch = 0, onauthrequired, ownedQty = null, marketStaleness = null, onsummary, banner, active = true, activity }: OrdersInput & { banner?: Snippet; activity?: ActivityLog } = $props();

  const controller = createOrdersController({ get transport() { return transport; }, get market() { return market; }, get ownedQty() { return ownedQty; }, get sessionEpoch() { return sessionEpoch; }, get onauthrequired() { return onauthrequired; }, get onsummary() { return onsummary; }, get active() { return active; } }, useDesktopServices());
  // Rows on screen but not freshly confirmed (refreshing, or the refresh
  // failed): they stay readable, and changes wait for a confirmed list.
  let unconfirmed = $derived(controller.phase !== 'done');
  // Retained quotes outlive the moment they describe once the view is left and
  // revisited, so the rail says when they were taken.
  let liveAt = $derived(controller.liveTop.checkedAt == null || controller.liveTop.quotes.size === 0 ? null
    : new Date(controller.liveTop.checkedAt).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' }));
  // Live checks report in the status strip's activity cell too, so a check
  // started here is still visible from another view.
  let liveActivity: number | null = null;
  $effect(() => {
    const phase = controller.liveTop.phase;
    const { done, total } = controller.liveTop.progress;
    untrack(() => {
      if (!activity) return;
      if (phase === 'running') {
        if (liveActivity == null) liveActivity = activity.begin(t('orders.liveProgress', { done, total }));
        else activity.progress(liveActivity, t('orders.liveProgress', { done, total }));
      } else if (liveActivity != null) {
        const id = liveActivity;
        liveActivity = null;
        if (phase === 'error') activity.finish(id, t('orders.liveFailedStatus'), 'bad');
        // Only a session change resets a running check; its result was discarded.
        else if (phase === 'idle') activity.finish(id, t('orders.liveStopped'));
        else activity.finish(id, controller.health.length ? t('orders.liveToFix', { count: controller.health.length }) : t('orders.liveNoIssues'));
      }
    });
  });
  onDestroy(() => controller.dispose());
  function focusIf(node: HTMLElement, should: boolean): void {
    if (should) node.focus();
  }
  // Editing, confirming and deleting replace the buttons in a row; focus
  // follows to the replacement so the keyboard user keeps their place.
  const rowButton = (id: string, cls: string) => () =>
    [...document.querySelectorAll<HTMLElement>(`.orders-table .${cls}[data-order]`)].find((el) => el.dataset.order === id);
  function editPrice(o: Parameters<typeof controller.startEdit>[0]) {
    controller.startEdit(o);
    focusLater(() => {
      const input = rowButton(o.id, 'price-input')() as HTMLInputElement | undefined;
      input?.select();
      return input;
    });
  }
  function cancelEdit(id: string) { controller.editingId = null; focusLater(rowButton(id, 'edit')); }
  async function saveEdit(o: Parameters<typeof controller.saveEdit>[0]) {
    await controller.saveEdit(o);
    if (controller.editingId !== o.id) focusLater(rowButton(o.id, 'edit'));
  }
  function cancelRemove(id: string) { controller.confirmId = null; focusLater(rowButton(id, 'delete')); }
  async function confirmRemove(o: Parameters<typeof controller.removeOne>[0], index: number) {
    await controller.removeOne(o);
    focusLater(() => {
      const rows = [...document.querySelectorAll<HTMLElement>('.orders-table .delete[data-order]')];
      return rowButton(o.id, 'delete')() ?? rows[Math.min(index, rows.length - 1)] ?? document.querySelector<HTMLElement>('.order-search');
    });
  }
</script>

<!-- One table for every listing. Health used to be a second panel that
     listed the same orders with different columns; it is now a column and a
     row state, with each fix beside the item it repairs. Live ask and bid are
     always reserved, so a live check fills cells instead of moving controls. -->
<section class="view-header">
  <h2>{t('orders.title')}</h2>
  <p class="lede">{t('orders.lede')}</p>
  <div class="vh-end">
    {#if controller.phase === 'done' && controller.orders.length > 0}
      <div class="ui-totals" role="group" aria-label={t('orders.summary')}>
        <div class="cell"><span class="k">{t('orders.listed')}</span><span class="v">{controller.orders.length}{#if controller.listedValue > 0}{' · '}{controller.listedValue.toLocaleString(formatLocale())}<span class="unit">p</span>{/if}</span></div>
        {#if controller.counts.issues > 0}<div class="cell attn"><span class="k">{t('orders.needsFixing')}</span><span class="v">{controller.counts.issues}</span></div>{/if}
      </div>
    {/if}
    <button
      class="btn primary"
      onclick={controller.checkLive}
      disabled={controller.phase !== 'done' || controller.orders.length === 0}
      use:waiting={controller.liveTop.phase === 'running'}
      title={t('orders.checkLiveTitle')}
    >
      {#if controller.liveTop.phase === 'running'}{t('orders.checkingLive', { done: controller.liveTop.progress.done, total: controller.liveTop.progress.total })}
      {:else if controller.liveTop.phase === 'done'}{t('orders.recheckLive')}
      {:else}{t('orders.checkLive')}{/if}
    </button>
  </div>
</section>
{@render banner?.()}

<section class="wrap tw orders" aria-label={t('orders.listingsLabel')}>
  <div class="rail">
    <h3>{t('orders.listings')}</h3>
    <span class="exp">
      {#if controller.phase === 'loading' || controller.phase === 'idle'}{t(controller.orders.length > 0 ? 'orders.refreshing' : 'orders.fetching')}
      {:else if controller.phase === 'locked'}{t('orders.unlockRequired')}
      {:else if controller.phase === 'error'}{t('orders.couldNotLoad')}
      {:else if controller.queue.length > 0}{t('orders.needAttention', { issues: controller.counts.issues, count: controller.orders.length })}{liveAt ? t('orders.livePricesFrom', { time: liveAt }) : ''}
      {:else if controller.liveTop.quotes.size > 0}{t('orders.noIssuesLive', { count: controller.orders.length })}{liveAt ? t('orders.at', { time: liveAt }) : ''}
      {:else if ownedQty}{t('orders.noIssuesScan', { count: controller.orders.length })}
      {:else}{t('orders.nothingFlagged')}
      {/if}
    </span>
    <span class="grow"></span>
    {#if controller.healthSummary.overpriced + controller.healthSummary.underbid > 1}
      <button class="btn" onclick={controller.fixAllPrices} disabled={unconfirmed} use:waiting={controller.fixAllBusy} title={t('orders.fixAllTitle')}>{t('orders.fixAll')}</button>
    {/if}
  </div>
  {#if controller.health.length > 0}
    <div class="bar">
      <span class="chips">
        {#if controller.healthSummary.overpriced}<span class="chip warn">{t('orders.chip.overpriced', { count: controller.healthSummary.overpriced })}</span>{/if}
        {#if controller.healthSummary.underbid}<span class="chip warn">{t('orders.chip.underbid', { count: controller.healthSummary.underbid })}</span>{/if}
        {#if controller.healthSummary.excessQty}<span class="chip">{t('orders.chip.excess', { count: controller.healthSummary.excessQty })}</span>{/if}
        {#if controller.healthSummary.notOwned}<span class="chip bad">{t('orders.chip.notOwned', { count: controller.healthSummary.notOwned })}</span>{/if}
      </span>
    </div>
  {/if}
  {#if controller.liveTop.phase === 'error' && controller.liveTop.error}
    <div class="line bad">{t('orders.liveFailed', { error: controller.liveTop.error })}</div>
  {/if}
  <div class="bar order-filters">
    <span class="ui-segmented" role="group" aria-label={t('orders.show')} use:roving>
      <button type="button" aria-pressed={controller.show === 'all'} onclick={() => (controller.show = 'all')}>{t('orders.filter.all')} <span class="n">{controller.counts.all}</span></button>
      <button type="button" aria-pressed={controller.show === 'sell'} onclick={() => (controller.show = 'sell')}>{t('orders.filter.sell')} <span class="n">{controller.counts.sell}</span></button>
      <button type="button" aria-pressed={controller.show === 'buy'} onclick={() => (controller.show = 'buy')}>{t('orders.filter.buy')} <span class="n">{controller.counts.buy}</span></button>
      <button type="button" aria-pressed={controller.show === 'hidden'} onclick={() => (controller.show = 'hidden')}>{t('orders.filter.hidden')} <span class="n">{controller.counts.hidden}</span></button>
      <button type="button" class:issues={controller.counts.issues > 0} aria-pressed={controller.show === 'issues'} onclick={() => (controller.show = 'issues')}>{t('orders.filter.issues')} <span class="n">{controller.counts.issues}</span></button>
    </span>
    <input class="input order-search" use:searchShortcut type="text" placeholder={t('orders.filterPlaceholder')} aria-label={t('orders.filterLabel')} bind:value={controller.nameFilter} />
    <button
      class="btn ghost"
      onclick={() => controller.bulkSetVisible(true)}
      disabled={controller.bulkBusy || unconfirmed || controller.orders.every((o) => o.visible == null || o.visible)}
      title={t('orders.allVisibleTitle')}
    >{t('orders.allVisible')}</button>
    <button
      class="btn ghost"
      onclick={() => controller.bulkSetVisible(false)}
      disabled={controller.bulkBusy || unconfirmed || controller.orders.every((o) => o.visible == null || !o.visible)}
      title={t('orders.allHiddenTitle')}
    >{t('orders.allHidden')}</button>
    <button class="btn" onclick={controller.loadOrders} use:waiting={controller.phase === 'loading'}>{t('orders.refresh')}</button>
  </div>

  {#if controller.phase === 'error'}
    <div class="line bad" role="alert">{t(controller.orders.length > 0 ? 'orders.refreshFailed' : 'orders.loadFailed', { error: controller.error ?? '' })}</div>
  {/if}
  {#if controller.phase === 'locked'}
    <div class="line"><span class="exp">{t('orders.unlockToSee')}</span></div>
  {:else if controller.phase === 'done' && controller.orders.length === 0}
    <div class="line"><span class="exp">{t('orders.none')}</span></div>
  {:else if controller.orders.length > 0}
    <div class="scroll" use:fitTable>
      <!-- Sized by content: numbers and buttons take what they need, Item and
           Health share the rest, so the table fits the panel instead of
           scrolling its last column out of view. -->
      <table class="tw orders-table">
        <thead>
          <tr>
            <th class="l">{t('orders.col.item')}</th>
            <th class="fix"><span class="sr-only">{t('orders.col.fix')}</span></th>
            <th data-drop="1">{t('orders.col.type')}</th>
            <th>{t('orders.col.qty')}</th>
            <th>{t('orders.col.lot')}</th>
            <th title={t('orders.col.askTitle')}>{t('orders.col.ask')}</th>
            <th data-drop="2" title={t('orders.col.bidTitle')}>{t('orders.col.bid')}</th>
            <th class="l health">{t('orders.col.health')}</th>
            <th>{t('orders.col.visible')}</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          {#each controller.shown as o, index (o.id)}
            {@const busy = controller.busyIds.has(o.id)}
            {@const live = controller.liveTop.quotes.size > 0 ? controller.liveForOrder(o) : null}
            {@const issues = controller.queue.filter((q) => q.id === o.id)}
            <tr class:busy class:attn={issues.length > 0} class:confirming={controller.confirmId === o.id || (controller.healthConfirmId === o.id && issues.length > 0)}>
              <td class="l"><span class="item-id">{#if o.slug}<ItemArt slug={o.slug} />{/if}<span>{controller.itemName(o)}</span></span></td>
              <td class="l act fix">
                {#each issues as q (q.key)}
                  {#if q.kind === 'health'}
                    {#if q.h.kind === 'not-owned' && controller.healthConfirmId === q.id}
                      <button class="btn xs bad" use:focusIf={true} onclick={() => controller.applyFix(q.h)} disabled={unconfirmed} use:waiting={busy} title={t('orders.confirmDelete')}>{t('orders.confirm')}</button>
                      <button class="btn xs x" onclick={() => controller.cancelDelete(q.id)} title={t('orders.cancel')} aria-label={t('orders.cancelDelete')}>×</button>
                    {:else}
                      <button class="btn xs" class:bad={q.h.kind === 'not-owned'} use:focusIf={controller.restoreFocusTo === q.id} onclick={() => controller.armOrFix(q.h)} disabled={unconfirmed} use:waiting={busy} title={controller.healthWhy(q.h)}>{controller.healthAction(q.h)}</button>
                    {/if}
                  {:else}
                    <button class="btn xs" onclick={() => controller.reprice(q.d)} disabled={unconfirmed} use:waiting={busy} title={t('orders.repriceTitle', { price: q.d.suggested })}>{t('orders.action.reprice')}</button>
                  {/if}
                {/each}
              </td>
              <td><span class="type" class:buy={o.side === 'buy'}>{o.side === 'sell' ? t('orders.side.sell') : o.side === 'buy' ? t('orders.side.buy') : o.side ?? '?'}</span></td>
              <td>{o.quantity ?? '?'}</td>
              <td class="price">
                {#if controller.editingId === o.id}
                  <input type="number" class="price-input" data-order={o.id} bind:value={controller.editValue} min="1" max={MAX_PLATINUM} aria-label={t('orders.newPrice', { item: controller.itemName(o) })}
                    use:waiting={busy}
                    onkeydown={(event) => {
                      if (event.isComposing) return;
                      if (event.key === 'Enter') { event.preventDefault(); if (!busy && !unconfirmed) void saveEdit(o); }
                      else if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); cancelEdit(o.id); }
                    }} />
                  <button class="btn xs" onclick={() => saveEdit(o)} disabled={unconfirmed} use:waiting={busy} aria-label={t('orders.savePrice', { item: controller.itemName(o) })}>{t('orders.save')}</button>
                  <button class="btn xs x" onclick={() => cancelEdit(o.id)} title={t('orders.cancel')} aria-label={t('orders.cancelEdit')}>×</button>
                {:else}
                  <span class="fg">{o.platinum}<span class="unit">p</span></span>
                  {#if (o.per_trade ?? 1) > 1}<span class="unit">{t('orders.units', { count: o.per_trade ?? 1 })}</span>{/if}
                  <button class="btn xs ghost edit" data-order={o.id} onclick={() => editPrice(o)} disabled={unconfirmed} use:waiting={busy} title={t('orders.editPrice')} aria-label={t('orders.editPriceFor', { item: controller.itemName(o) })}>✎</button>
                {/if}
              </td>
              <td>{#if live && !live.error && live.low_sell != null}{Number(live.low_sell.toFixed(2))}<span class="unit">p</span>{:else}<span class="muted">-</span>{/if}</td>
              <td>{#if live && !live.error && live.top_buy != null}{Number(live.top_buy.toFixed(2))}<span class="unit">p</span>{:else}<span class="muted">-</span>{/if}</td>
              <td class="l health-cell">
                {#each issues as q (q.key)}
                  {#if q.kind === 'health'}
                    <span class="issue" class:warn={q.h.kind === 'overpriced' || q.h.kind === 'underbid'} class:bad={q.h.kind === 'not-owned'} title={controller.healthWhy(q.h)}>
                      {#if q.h.kind === 'overpriced' || q.h.kind === 'underbid'}
                        <span class="to">{q.h.current}p → <b>{q.h.suggested}p</b></span>
                        {t(q.h.kind === 'overpriced' ? 'orders.health.overpriced' : 'orders.health.underbid')}
                      {:else if q.h.kind === 'excess-qty'}
                        <span class="to">×{q.h.current} → <b>×{q.h.suggested}</b></span>
                        {t('orders.health.excess')}
                      {:else}
                        <span class="to">×{q.h.current}</span>
                        {t('orders.health.notOwned')}
                      {/if}
                    </span>
                  {:else}
                    <span class="issue" class:warn={q.d.kind === 'overpriced'} title={controller.driftWhy(q.d)}>
                      <span class="to">{Number(q.d.listed.toFixed(2))}p → <b>{q.d.suggested}p</b></span>
                      {t(q.d.kind === 'overpriced' ? 'orders.drift.over' : 'orders.drift.under', { pct: Math.round(q.d.delta_pct * 100) })}
                      {#if q.d.thin}<span class="tag thin" title={t('orders.thinTitle', { volume: LIQUID_VOL })}>{t('orders.thin')}</span>{/if}
                    </span>
                  {/if}
                {:else}<span class="muted">-</span>{/each}
              </td>
              <td>
                <button
                  class="visbtn {o.visible ? 'on' : 'off'}"
                  onclick={() => controller.toggleVisible(o)}
                  disabled={unconfirmed || o.visible == null}
                  use:waiting={busy}
                  title={t(o.visible == null ? 'orders.visUnknownTitle' : o.visible ? 'orders.makeHiddenTitle' : 'orders.makeVisibleTitle')}
                  aria-label={t(o.visible == null ? 'orders.visUnknown' : o.visible ? 'orders.visOn' : 'orders.visOff', { item: controller.itemName(o) })}
                ><span class="vis" class:off={!o.visible}>{o.visible == null ? '?' : t(o.visible ? 'orders.on' : 'orders.off')}</span></button>
              </td>
              <td class="act">
                {#if controller.confirmId === o.id}
                  <!-- svelte-ignore a11y_no_static_element_interactions (Escape backs out of the confirmation.) -->
                  <span class="confirm-pair" onkeydown={(event) => { if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); cancelRemove(o.id); } }}>
                    <button class="btn xs bad" use:focusIf={true} onclick={() => confirmRemove(o, index)} disabled={unconfirmed} use:waiting={busy} title={t('orders.confirmDelete')} aria-label={t('orders.confirmDeleteItem', { item: controller.itemName(o) })}>{t('orders.confirm')}</button>
                    <button class="btn xs x" onclick={() => cancelRemove(o.id)} title={t('orders.cancel')} aria-label={t('orders.cancelDelete')}>×</button>
                  </span>
                {:else}
                  <button class="btn xs x delete" data-order={o.id} onclick={() => controller.removeOne(o)} disabled={unconfirmed} use:waiting={busy} title={t('orders.delete')} aria-label={t('orders.deleteItem', { item: controller.itemName(o) })}>✕</button>
                {/if}
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
    {#if controller.shown.length === 0}
      <div class="line"><span class="exp">{t('orders.noMatch')}</span> <button class="btn xs ghost" onclick={() => { controller.show = 'all'; controller.nameFilter = ''; }}>{t('orders.clear')}</button></div>
    {/if}
    {#if controller.liveTop.quotes.size === 0 && controller.drifted.length > 0}
      <div class="line">
        <span class="exp">{t('orders.snapshotNote', { age: marketStaleness ?? t('orders.unknownTime') })}<button class="linkish" onclick={controller.checkLive} use:waiting={controller.liveTop.phase === 'running'}>{t('orders.checkLiveLink')}</button>{t('orders.forExact')}</span>
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
  /* A row with something to fix carries the caution edge; the words in the
     Health cell say what, the edge only repeats it. */
  tr.attn td:first-child { box-shadow: inset 3px 0 0 var(--warn); }
  /* Item and Health split the slack; Health wraps rather than widening the
     table. The minimums leave room for a classic 15px scrollbar (Windows) at
     1200px with Delete's Confirm open. */
  .orders table.tw th.l { width: 55%; min-width: 8rem; }
  .orders table.tw th.health { width: 45%; min-width: 10rem; }
  /* Beats the shared td.l wrapping, which stacked the fix buttons over the item. */
  .orders table.tw td.fix { white-space: nowrap; overflow-wrap: normal; text-align: left; }
  td.fix .btn + .btn { margin-left: var(--s1); }
  .orders table.tw td.act { white-space: nowrap; }
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
