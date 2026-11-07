<script lang="ts">
  import { fitTable } from '../../ui/table-fit';
  import { onMount, onDestroy } from 'svelte';
  import { useDesktopServices } from '../shell/desktop-context';
  import { createListingReview, type ListingReviewInput } from './listing-review.svelte';
  import { plat, ownedBreakdown, leveledNoteTitle, keptNoteTitle } from '../../ui/format';
  import { orderUnitPrice } from '../../domain/order-prices';
  import { MAX_PLATINUM } from '../../domain/limits';
  import DialogHeader from '../../ui/DialogHeader.svelte';
  import { busy, focusLater, focusView } from '../../ui/keyboard-actions';
  import Rich from '../../ui/Rich.svelte';
  import { t } from '../../i18n/i18n.svelte';
  let { open = $bindable(false), rows, transport, onauthrequired, onclose, onsent, onvisible, sendThrough = (send) => send(), listingBlockReason = null, onrecheck, listingActionLabel, currentSnapshotId, sessionEpoch = 0 }: Omit<ListingReviewInput, 'open' | 'sendThrough'> & { open?: boolean; sendThrough?: ListingReviewInput['sendThrough'] } = $props();

  const controller = createListingReview({ get transport() { return transport; }, get open() { return open; }, set open(value) { open = value; }, get rows() { return rows; }, get listingBlockReason() { return listingBlockReason; }, get currentSnapshotId() { return currentSnapshotId; }, get sessionEpoch() { return sessionEpoch; }, get onauthrequired() { return onauthrequired; }, get onclose() { return onclose; }, get onsent() { return onsent; }, get onvisible() { return onvisible; }, get sendThrough() { return sendThrough; } }, useDesktopServices());
  onMount(() => controller.start());
  // Escape after editing asks first: many people press it to leave one field,
  // and it used to throw every price and quantity away.
  let edited = $state(false);
  let confirmDiscard = $state(false);
  let dialogNode = $state<HTMLElement>();
  $effect(() => { if (!open) { edited = false; confirmDiscard = false; } });
  function escape() {
    if (controller.phase === 'review' && edited && !confirmDiscard) {
      confirmDiscard = true;
      focusLater(() => dialogNode?.querySelector<HTMLElement>('.discard .keep'));
      return;
    }
    if (confirmDiscard) { keepEditing(); return; }
    controller.close();
  }
  function keepEditing() { confirmDiscard = false; focusLater(() => dialogNode?.querySelector<HTMLElement>('input.price:not(:disabled)')); }
  // Each phase replaces the controls; focus moves to what the phase is about,
  // so the outcome of a send is read out and the next Tab is in the dialog.
  let shownPhase = '';
  $effect(() => {
    const phase = controller.phase;
    if (!open || phase === shownPhase) return;
    shownPhase = phase;
    if (phase === 'sending') focusLater(() => dialogNode?.querySelector<HTMLElement>('.stop-sending'));
    else if (phase === 'results' || phase === 'error') focusLater(() => dialogNode?.querySelector<HTMLElement>('.outcome'));
  });
  onDestroy(() => controller.dispose());
  function reviewFocus(node: HTMLElement) {
    const previous = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    let mounted = true;
    const controls = () => [...node.querySelectorAll<HTMLElement>('button, input, select, textarea, a[href], [tabindex]')]
      .filter(el => !el.matches(':disabled, [tabindex="-1"]') && el.getClientRects().length > 0);
    // Prices are what a review is for, so it opens on the first one.
    queueMicrotask(() => { if (mounted) (node.querySelector<HTMLElement>('input.price:not(:disabled)') ?? controls()[0])?.focus(); });
    const onKey = (event: KeyboardEvent) => {
      if (event.defaultPrevented || document.querySelector('dialog[open]')) return;
      if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); escape(); }
      if (event.key !== 'Tab') return;
      const available = controls();
      const first = available[0];
      const last = available.at(-1);
      if (!node.contains(document.activeElement)) { event.preventDefault(); first?.focus(); }
      else if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last?.focus(); }
      else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first?.focus(); }
    };
    // A refresh disables its focused button briefly; browsers can move focus
    // to body. Keep Escape/Tab usable without closing an auth dialog above us.
    document.addEventListener('keydown', onKey);
    return { destroy() {
      mounted = false;
      document.removeEventListener('keydown', onKey);
      // An authentication dialog may have taken focus during the review handoff.
      if (previous?.isConnected && (node.contains(document.activeElement) || document.activeElement === document.body)) previous.focus();
      // A pick's List button becomes its receipt once listed; focus goes to
      // that row's receipt (Make visible, My orders) instead of the page.
      else if (previous?.dataset.pick && (node.contains(document.activeElement) || document.activeElement === document.body)) {
        const key = previous.dataset.pick;
        requestAnimationFrame(() => {
          const receipt = [...document.querySelectorAll<HTMLElement>('[data-pick]')].find((el) => el.dataset.pick === key);
          if (receipt) receipt.focus(); else focusView();
        });
      }
    } };
  }

</script>

{#if open}
  <div class="backdrop" role="dialog" aria-modal="true" aria-labelledby="rm-title" use:reviewFocus bind:this={dialogNode}>
    <div class="modal" class:session={controller.hasSession}>
      <DialogHeader titleId="rm-title" title={t('review.title')} onclose={controller.close} />

      {#if controller.reviewBlockReason && controller.phase === 'review'}<div class="ui-notice" data-tone="warn" role="status">{controller.reviewBlockReason} {#if listingBlockReason && onrecheck}<button class="btn" onclick={onrecheck}>{listingActionLabel ?? t('sell.checkListings')}</button>{:else if !listingBlockReason}<button class="btn" onclick={controller.close}>{t('review.closeReview')}</button>{/if}</div>{/if}
      {#if controller.phase === 'review'}
        <p class="lead">
          <Rich text={t(controller.hasSession ? 'review.leadSession' : 'review.lead')} />
        </p>

        {#if controller.hasSession}
          {#if controller.allocationProblem}<p class="ui-notice" data-tone="warn" role="alert">{controller.allocationProblem}</p>{/if}
          <p class="ui-notice" data-tone={controller.canSubmit ? 'good' : 'warn'}>
            {t('review.tradeBudget', { trades: controller.estimatedTrades ?? t('review.invalidLot'), budget: controller.plan[0]?.session?.budget ?? '', remaining: controller.sessionRemaining ?? t('review.unknown') })}
          </p>
          {#if controller.sessionProblem}<p class="ui-notice" data-tone="warn">{controller.sessionProblem}</p>{/if}
          {#if controller.plan.some(r => r.include && r.quantity > r.sellable)}
            <p class="ui-notice" data-tone="warn">{t('review.overSellable')}</p>
          {/if}
          {#if controller.networkError}<p class="ui-notice" data-tone="warn" role="alert">{controller.networkError}</p>{/if}
          <button class="btn ghost" onclick={() => controller.refreshReviewOrders(false)} use:busy={controller.ordersBusy}>
            {t(controller.ordersBusy ? 'review.checkingOrders' : 'review.refreshOrders')}
          </button>
        {/if}

        <div class="bulkrow">
          <button class="btn ghost" onclick={() => { edited = true; controller.setAll(true); }}>{t('review.selectAll')}</button>
          <button class="btn ghost" onclick={() => { edited = true; controller.setAll(false); }}>{t('review.deselectAll')}</button>
          <span class="spacer"></span>
          <button
            class="btn ghost live-btn"
            onclick={controller.checkLivePrices}
            disabled={controller.selectedCount === 0}
            use:busy={controller.liveTop.phase === 'running'}
            title={t('review.liveTitle')}
          >
            {#if controller.liveTop.phase === 'running'}
              {t('review.checkingLive', { done: controller.liveTop.progress.done, total: controller.liveTop.progress.total })}
            {:else if controller.liveTop.phase === 'done'}
              {t('review.recheckLive')}
            {:else}
              {t('review.checkLive')}
            {/if}
          </button>
          {#if controller.liveTop.phase === 'done' && controller.liveTop.quotes.size > 0}
            <button class="btn ghost" onclick={controller.useLiveAll} title={t('review.matchAsksTitle')}>{t('review.matchAsks')}</button>
          {/if}
          {#if controller.liveTop.phase === 'error' && controller.liveTop.error}
            <span class="live-err">{controller.liveTop.error}</span>
          {/if}
        </div>

        <div class="scroll" use:fitTable oninput={() => (edited = true)} onchange={() => (edited = true)}>
          <table>
            <thead>
              <tr>
                <th><span class="visually-hidden">{t('review.include')}</span></th>
                <th>{t('review.col.item')}</th>
                <th>{t('review.col.qty')}</th>
                {#if controller.hasSession}<th>{t('review.col.unitsPerTrade')}</th><th>{t('review.col.change')}</th>{/if}
                <th>{t('review.col.owned')}</th>
                <th>{t('review.col.price')}</th>
                <th>{t('review.col.avg')}</th>
                <th title={t('review.col.liveTitle')}>{t('review.col.live')}</th>
                <th title={t('review.col.rankTitle')}>{t('review.col.rank')}</th>
                <th>{t('review.col.subtotal')}</th>
              </tr>
            </thead>
            <tbody>
              {#each controller.plan as row, i (row.key)}
                {@const live = controller.liveFor(row)}
                {@const v = controller.liveVerdict(row)}
                <tr class:dim={!row.include}>
                  <td><input type="checkbox" aria-label={t('review.includeItem', { item: row.name })} bind:checked={controller.plan[i].include} /></td>
                  <td>{row.name}</td>
                  <td>
                    <input
                      type="number"
                      aria-label={t('review.quantityFor', { item: row.name })}
                      min="1"
                      max={row.sellable}
                      bind:value={controller.plan[i].quantity}
                      disabled={!row.include}
                    />
                  </td>
                  {#if controller.hasSession}
                    <td><input type="number" aria-label={t('review.unitsFor', { item: row.name })} min="1" max={row.bulk ? 6 : 1}
                      step="1" bind:value={controller.plan[i].per_trade} disabled={!row.include} /></td>
                    <td>
                      {#if row.reviewed_order?.state === 'existing'}
                        {@const prior = row.reviewed_order}
                        <span>{t(prior.visible ? 'review.updateVisible' : 'review.updateHidden')}</span><br />
                        <span>{t('review.qtyChange', { from: prior.quantity, to: row.quantity, fromPrice: Number((orderUnitPrice(prior.platinum, prior.per_trade) ?? 0).toFixed(2)), toPrice: row.platinum })}</span><br />
                        <span>{t('review.lotChange', { from: prior.platinum, to: row.platinum * row.per_trade })}</span><br />
                        <span>{t(prior.visible ? 'review.unitsChangeVisible' : 'review.unitsChangeHidden', { from: prior.per_trade ?? 1, to: row.per_trade })}</span>
                      {:else if row.reviewed_order?.state === 'new'}
                        {t('review.newListing', { quantity: row.quantity, price: row.platinum, units: row.per_trade, lot: row.platinum * row.per_trade })}
                      {:else}{t('review.notConfirmed')}{/if}
                    </td>
                  {/if}
                  <td class="muted">
                    {#if row.sellable < row.owned}
                      {@const bd = ownedBreakdown(row.owned, row.sellable, row.leveled)}
                      {t('review.owned', { count: row.owned })}{#if bd.leveledPart > 0}{' · '}<span class="leveled-note" title={leveledNoteTitle()}>{t('table.leveled', { count: bd.leveledPart })}</span>{/if}{#if bd.keptPart > 0}{' · '}<span class="kept-note" title={keptNoteTitle(bd.keptPart)}>{t('table.kept', { count: bd.keptPart })}</span>{/if}
                    {:else}
                      {t('review.owned', { count: row.owned })}
                    {/if}
                  </td>
                  <td>
                    <input
                      type="number"
                      class="price"
                      aria-label={t('review.priceFor', { item: row.name })}
                      min="5"
                      max={MAX_PLATINUM}
                      bind:value={controller.plan[i].platinum}
                      disabled={!row.include}
                      class:off={row.include && controller.priceOff(row)}
                      aria-invalid={row.include && controller.priceOff(row) ? 'true' : undefined}
                      title={row.include && controller.priceOff(row) ? t('review.priceOff', { avg: row.avg.toFixed(0) }) : undefined}
                    />
                  </td>
                  <td class="muted">{plat(row.avg)}</td>
                  <td class="live-cell" class:above={v === 'above'} class:belowbid={v === 'below-bid'}>
                    {#if !live}
                      <span class="muted">·</span>
                    {:else if live.error}
                      <span class="muted" title={live.error}>{t('review.na')}</span>
                    {:else}
                      {#if live.low_sell != null}
                        <button class="linkish" onclick={() => controller.useLive(i)} disabled={!row.include}
                          title={t('review.onlineAsks', { asks: live.sells.join(', '), price: live.low_sell })}>{live.low_sell}p</button>
                      {:else}<span class="muted" title={t('review.noSellers')}>{t('review.noAsk')}</span>{/if}
                      <span class="muted"> / </span>
                      {#if live.top_buy != null}
                        <span title={t('review.onlineBids', { bids: live.buys.join(', ') })}>{live.top_buy}p<span class="visually-hidden">{t('review.topBidSr', { bids: live.buys.join(', ') })}</span></span>
                      {:else}<span class="muted" title={t('review.noBuyers')}>{t('review.noBid')}</span>{/if}
                      {#if v === 'above'}<span class="verdict" title={t('review.aboveAsk')}>▲<span class="visually-hidden">{t('review.aboveAskSr')}</span></span>{/if}
                      {#if v === 'below-bid'}<span class="verdict" title={t('review.belowBid')}>▼<span class="visually-hidden">{t('review.belowBidSr')}</span></span>{/if}
                    {/if}
                  </td>
                  <td>
                    <input
                      type="number"
                      aria-label={t('review.rankFor', { item: row.name })}
                      min="0"
                      max="10"
                      class="rank"
                      bind:value={controller.plan[i].rank}
                      disabled={!row.include || !!row.session}
                    />
                  </td>
                  <td class="right">{plat(row.platinum * row.quantity)}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>

        {#if controller.marketAccess.message}<p class="ui-notice" data-tone="warn" role="status">{controller.marketAccess.message}</p>{/if}
        {#if confirmDiscard}
          <div class="ui-notice discard" data-tone="warn" role="alertdialog" aria-labelledby="rm-discard">
            <span id="rm-discard">{t('review.discardAsk')}</span>
            <button class="btn keep" onclick={keepEditing}>{t('review.keepEditing')}</button>
            <button class="btn" onclick={controller.close}>{t('review.discard')}</button>
          </div>
        {/if}
        <footer>
          <div class="totals">
            <span><Rich text={t('review.items', { count: controller.selectedCount })} /></span>
            <span><Rich text={t('review.platTotal', { total: plat(controller.totalPlat) })} /></span>
            {#if controller.selectedCount > 50}
              <span class="warn">{t('review.batchCap')}</span>
            {/if}
          </div>
          <div class="actions">
            <button class="btn ghost" onclick={controller.close}>{t('review.cancel')}</button>
            <button class="btn primary" onclick={controller.send} disabled={!controller.canSubmit || controller.validatingSend || controller.marketAccess.mutationsBlocked}>
              {t('review.send', { count: controller.selectedCount })}
            </button>
          </div>
        </footer>
      {:else if controller.phase === 'sending'}
        <p class="lead">
          {t('review.sendingLead')}
        </p>
<div class="spinner">{controller.cancellationRequested ? t('review.finishing') : controller.marketAccess.message ?? t('review.sending')}</div>
        {#if controller.networkError}<p class="ui-notice" data-tone="bad">{controller.networkError}</p>{/if}
        <button class="btn stop-sending" onclick={controller.stopSending} use:busy={controller.cancellationRequested}>{t('review.stop')}</button>
      {:else if controller.phase === 'results'}
        {#if controller.durabilityError}
          <p class="ui-notice" data-tone="bad" role="alert">{controller.durabilityError}</p>
        {/if}
        <p class="lead outcome" tabindex="-1">
          {t(controller.durabilityError ? 'review.needsAttention' : controller.pendingCount > 0 ? 'review.interrupted' : 'review.done')} <span class="ok">{t('review.created', { count: controller.okCount })}</span>{#if controller.updatedCount > 0}{' · '}<span class="ok">{t('review.updated', { count: controller.updatedCount })}</span>{/if}{#if controller.errCount > 0}{' · '}<span class="bad">{t('review.failed', { count: controller.errCount })}</span>{/if}{#if controller.pendingCount > 0}{' · '}<span class="warn">{t('review.savedForResume', { count: controller.pendingCount })}</span>{/if}.
          <Rich text={t(controller.visibilityDone ? 'review.nowVisible' : 'review.startHidden')} />
          {t('review.updatedKeep')}
        </p>
        <div class="scroll" use:fitTable>
          <table>
            <thead><tr><th></th><th>{t('review.col.item')}</th><th>{t('review.col.detail')}</th></tr></thead>
            <tbody>
              {#each controller.serverResults as r, i (i)}
                <tr>
                  <td class:ok={r.status === 'ok'} class:bad={r.status === 'error'} class:warn={r.status === 'pending' || r.status === 'uncertain_mutation'}>
                    {r.status === 'ok' ? '✓' : r.status === 'pending' || r.status === 'uncertain_mutation' ? '…' : '✗'}
                  </td>
                  <td>
                    <span class="item-name">{controller.planNameBySlug.get(r.slug) ?? r.slug}</span>
                    <span class="item-slug">{r.slug}</span>
                  </td>
                  <td class="muted">{r.message ?? r.order_id ?? ''}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
        {#if controller.visibilityResults.length > 0}
          <p class="lead">
            {t('review.visibilityToggled')} <span class="ok">{t('review.visibleCount', { count: controller.visibleOkCount })}</span>{#if controller.visibleErrCount > 0}{' · '}<span class="bad">{t('review.failed', { count: controller.visibleErrCount })}</span>{/if}.
          </p>
        {/if}

        <footer>
          <div></div>
          <div class="actions">
            {#if controller.okCount > 0 && !controller.visibilityDone}
              <button class="btn primary" onclick={controller.makeAllVisible} use:busy={controller.visibilityBusy}>
                {controller.visibilityBusy ? t('review.makingVisible') : t('review.makeVisible', { count: controller.okCount })}
              </button>
            {/if}
            <button class={controller.visibilityDone ? 'btn primary' : 'btn ghost'} onclick={controller.close}>{t('review.doneButton')}</button>
          </div>
        </footer>
      {:else if controller.phase === 'error'}
        <p class="lead bad outcome" tabindex="-1" role="alert">{controller.networkError}</p>
        <footer>
          <div></div>
          <div class="actions">
            <button class="btn ghost" onclick={controller.close}>{t('review.cancel')}</button>
            <button class="btn primary" onclick={() => (controller.phase = 'review')}>{t('review.backToReview')}</button>
          </div>
        </footer>
      {/if}
    </div>
  </div>
{/if}

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: var(--scrim);
    backdrop-filter: blur(2px);
    display: grid;
    place-items: center;
    z-index: var(--layer-modal);
    padding: clamp(8px, 3vw, 24px);
  }
  .modal {
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    width: min(900px, 100%);
    max-height: calc(100dvh - clamp(16px, 6vw, 48px));
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }
  .modal.session { width: min(74rem, 100%); overflow-y: auto; }
  .modal.session > :global(*) { flex-shrink: 0; }
  .modal.session .scroll { min-height: 12rem; max-height: 45dvh; flex-shrink: 0; }
  .modal.session table { min-width: 70rem; }
  .lead {
    padding: 14px 18px 0;
    margin: 0;
    font-size: var(--text-control);
    color: var(--muted);
    line-height: 1.5;
    max-width: 80ch;
  }
  .lead.bad { color: var(--bad); }
  .outcome:focus { outline: none; }
  .discard { display: flex; flex-wrap: wrap; align-items: center; gap: var(--s2); }
  .discard > span { flex: 1 1 14rem; }
  .lead :global(b) { color: var(--fg); }
  .bulkrow {
    display: flex;
    gap: 8px;
    padding: 8px 18px 0;
    align-items: center;
    flex-wrap: wrap;
  }
  .bulkrow .spacer { flex: 1; }
  .live-err { color: var(--bad); font-size: var(--text-caption); }
  td.live-cell { white-space: nowrap; font-variant-numeric: tabular-nums; }
  td.live-cell.above .verdict { color: var(--warn); margin-left: 4px; }
  td.live-cell.belowbid .verdict { color: var(--warn); margin-left: 4px; }
  button.linkish {
    background: none;
    border: 0;
    padding: 0;
    color: var(--fg);
    text-decoration: underline dotted;
    cursor: pointer;
    font: inherit;
  }
  button.linkish:hover:not(:disabled) { color: var(--accent, var(--fg)); }
  button.linkish:disabled { cursor: default; text-decoration: none; color: var(--muted); }
  .scroll {
    overflow: auto;
    min-height: 5rem;
    margin: 12px 0;
    border-top: 1px solid var(--border);
    border-bottom: 1px solid var(--border);
  }
  table {
    width: 100%;
    min-width: 44rem;
    border-collapse: collapse;
    font-variant-numeric: tabular-nums;
  }
  th, td {
    padding: 7px 12px;
    text-align: left;
    border-bottom: 1px solid var(--border);
    font-size: var(--text-control);
  }
  th {
    background: var(--panel-2);
    font-weight: 600;
    color: var(--muted);
    letter-spacing: 0.04em;
    text-transform: uppercase;
    font-size: var(--text-caption);
    position: sticky;
    top: 0;
  }
  td.right { text-align: right; }
  td.muted { color: var(--muted); }
  tr.dim td { color: var(--muted); }
  .item-name { color: var(--fg); }
  .item-slug {
    color: var(--muted);
    font-family: var(--font-mono);
    font-size: var(--text-caption);
    margin-left: 6px;
  }
  /* Leveled gear is a harder constraint than the Keep-copies reserve - the
     game itself won't let you trade it - so it gets the same warm tint
     ResultsTable uses for its owned-column note. */
  .leveled-note { color: var(--warn); }
  .kept-note { color: var(--muted); }
  input[type="number"] {
    font: inherit;
    font-family: var(--font-mono);
    font-size: var(--text-control);
    width: 64px;
    background: var(--panel-2);
    border: 1px solid var(--border);
    color: var(--fg);
    border-radius: var(--radius-ctl);
    padding: 3px 6px;
  }
  input[type="number"]:disabled { color: var(--muted); background: var(--panel-2); }
  input[type="number"].rank { width: 46px; }
  input[type="number"].off { border-color: var(--warn); }
  footer {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 12px 18px;
    border-top: 1px solid var(--border);
    gap: 12px;
    flex-wrap: wrap;
  }
  .totals {
    display: flex;
    gap: 18px;
    flex-wrap: wrap;
    font-size: var(--text-control);
    color: var(--muted);
  }
  .totals :global(b) { color: var(--fg); font-weight: 600; }
  .totals .warn { color: var(--warn); }
  .actions { display: flex; gap: 8px; flex-wrap: wrap; }
  td.ok { color: var(--good); font-weight: 600; }
  td.bad { color: var(--bad); font-weight: 600; }
  .ok { color: var(--good); }
  .bad { color: var(--bad); }
  .spinner {
    padding: 32px;
    text-align: center;
    color: var(--muted);
  }
  @media (max-width: 35rem) {
    .lead {
      max-height: 7rem;
      overflow-y: auto;
      padding: var(--s3) var(--s3) 0;
    }
    .bulkrow { padding-right: var(--s3); padding-left: var(--s3); }
    .scroll {
      flex: 1 1 7rem;
      min-height: 5rem;
      margin: var(--s2) 0;
    }
    footer { padding: var(--s2) var(--s3); }
  }
</style>
