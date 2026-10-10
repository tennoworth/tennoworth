<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { useDesktopServices } from '../../ui/desktop-context';
  import { createListingReview, type ListingReviewInput } from './listing-review.svelte';
  import { plat, ownedBreakdown, LEVELED_NOTE_TITLE, keptNoteTitle } from '../../ui/format';
  import { orderUnitPrice } from '../../domain/order-prices';
  import { MAX_PLATINUM } from '../../domain/limits';
  import DialogHeader from '../../ui/DialogHeader.svelte';
  let { open = $bindable(false), rows, transport, onauthrequired, onclose, onsent, onvisible, sendThrough = (send) => send(), listingBlockReason = null, onrecheck, listingActionLabel = 'Check WFM listings', currentSnapshotId }: Omit<ListingReviewInput, 'open' | 'sendThrough'> & { open?: boolean; sendThrough?: ListingReviewInput['sendThrough'] } = $props();

  const controller = createListingReview({ get transport() { return transport; }, get open() { return open; }, set open(value) { open = value; }, get rows() { return rows; }, get listingBlockReason() { return listingBlockReason; }, get currentSnapshotId() { return currentSnapshotId; }, get onauthrequired() { return onauthrequired; }, get onclose() { return onclose; }, get onsent() { return onsent; }, get onvisible() { return onvisible; }, get sendThrough() { return sendThrough; } }, useDesktopServices());
  onMount(() => controller.start());
  onDestroy(() => controller.dispose());
  function reviewFocus(node: HTMLElement) {
    const previous = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    let mounted = true;
    const controls = () => [...node.querySelectorAll<HTMLElement>('button, input, select, textarea, a[href], [tabindex]')]
      .filter(el => !el.matches(':disabled, [tabindex="-1"]') && el.getClientRects().length > 0);
    queueMicrotask(() => { if (mounted) controls()[0]?.focus(); });
    const onKey = (event: KeyboardEvent) => {
      if (event.defaultPrevented || document.querySelector('dialog[open]')) return;
      if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); controller.close(); }
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
    } };
  }

</script>

{#if open}
  <div class="backdrop" role="dialog" aria-modal="true" aria-labelledby="rm-title" use:reviewFocus>
    <div class="modal" class:session={controller.hasSession}>
      <DialogHeader titleId="rm-title" title="List on warframe.market" onclose={controller.close} />

      {#if controller.reviewBlockReason && controller.phase === 'review'}<div class="ui-notice" data-tone="warn" role="status">{controller.reviewBlockReason} {#if listingBlockReason && onrecheck}<button class="btn" onclick={onrecheck}>{listingActionLabel}</button>{:else if !listingBlockReason}<button class="btn" onclick={controller.close}>Close review</button>{/if}</div>{/if}
      {#if controller.phase === 'review'}
        <p class="lead">
          {#if controller.hasSession}
          New listings start <strong>hidden</strong>. Existing listings keep their visibility.
          Quantities replace current listing totals; rank stays at the verified unranked tier.
          {:else}
          Review every row. Default price is the estimated clearing price -
          the lowest live ask, sanity-clamped against the recent median so a
          lone troll listing can't set it (floored at 5p). Rank 0 = unranked;
          set a rank only if you're listing a leveled copy. New listings go up
          <strong>hidden</strong>. Updates replace the existing quantity and preserve visibility.
          {/if}
        </p>

        {#if controller.hasSession}
          {#if controller.allocationProblem}<p class="ui-notice" data-tone="warn" role="alert">{controller.allocationProblem}</p>{/if}
          <p class="ui-notice" data-tone={controller.canSubmit ? 'good' : 'warn'}>
            {controller.estimatedTrades ?? 'Invalid quantity / lot'} estimated trades / {controller.plan[0]?.session?.budget} budget · {controller.sessionRemaining ?? 'unknown'} remaining.
            Quantities must divide evenly by units per trade. Posting does not spend the game allowance.
          </p>
          {#if controller.sessionProblem}<p class="ui-notice" data-tone="warn">{controller.sessionProblem}</p>{/if}
          {#if controller.plan.some(r => r.include && r.quantity > r.sellable)}
            <p class="ui-notice" data-tone="warn">A selected quantity exceeds the confirmed sellable count. Your edits are retained; reduce the quantity or scan again.</p>
          {/if}
          {#if controller.networkError}<p class="ui-notice" data-tone="warn" role="alert">{controller.networkError}</p>{/if}
          <button class="btn ghost" onclick={() => controller.refreshReviewOrders(false)} disabled={controller.ordersBusy}>
            {controller.ordersBusy ? 'Checking existing orders…' : 'Refresh existing orders'}
          </button>
        {/if}

        <div class="bulkrow">
          <button class="btn ghost" onclick={() => controller.setAll(true)}>Select all</button>
          <button class="btn ghost" onclick={() => controller.setAll(false)}>Deselect all</button>
          <span class="spacer"></span>
          <button
            class="btn ghost live-btn"
            onclick={controller.checkLivePrices}
            disabled={controller.liveTop.phase === 'running' || controller.selectedCount === 0}
            title="Ask warframe.market for the ≤5 best online asks and bids for each selected row's exact rank / refinement, right now. Shares market access with other activity; large batches can take time."
          >
            {#if controller.liveTop.phase === 'running'}
              Checking live prices… {controller.liveTop.progress.done}/{controller.liveTop.progress.total}
            {:else if controller.liveTop.phase === 'done'}
              Re-check live prices
            {:else}
              Check live prices
            {/if}
          </button>
          {#if controller.liveTop.phase === 'done' && controller.liveTop.quotes.size > 0}
            <button class="btn ghost" onclick={controller.useLiveAll} title="Set every selected row's price to its live lowest online ask (match it - no undercutting).">Match lowest asks</button>
          {/if}
          {#if controller.liveTop.phase === 'error' && controller.liveTop.error}
            <span class="live-err">{controller.liveTop.error}</span>
          {/if}
        </div>

        <div class="scroll">
          <table>
            <thead>
              <tr>
                <th></th>
                <th>Item</th>
                <th>Qty</th>
                {#if controller.hasSession}<th>Units / trade</th><th>Existing → proposed</th>{/if}
                <th>Owned</th>
                <th>Price (p)</th>
                <th>Avg</th>
                <th title="Live lowest online ask / highest online bid for this exact rank or refinement (after “Check live prices”). Click a value to use it.">Live ask / bid</th>
                <th title="Mod/arcane rank of the copies you're listing. 0 = unranked (dupe stacks). Ignored for items WFM doesn't rank.">Rank</th>
                <th>Subtotal</th>
              </tr>
            </thead>
            <tbody>
              {#each controller.plan as row, i (row.key)}
                {@const t = controller.liveFor(row)}
                {@const v = controller.liveVerdict(row)}
                <tr class:dim={!row.include}>
                  <td><input type="checkbox" bind:checked={controller.plan[i].include} /></td>
                  <td>{row.name}</td>
                  <td>
                    <input
                      type="number"
                      min="1"
                      max={row.sellable}
                      bind:value={controller.plan[i].quantity}
                      disabled={!row.include}
                    />
                  </td>
                  {#if controller.hasSession}
                    <td><input type="number" aria-label={`Units per trade for ${row.name}`} min="1" max={row.bulk ? 6 : 1}
                      step="1" bind:value={controller.plan[i].per_trade} disabled={!row.include} /></td>
                    <td>
                      {#if row.reviewed_order?.state === 'existing'}
                        {@const prior = row.reviewed_order}
                        <span>Update {prior.visible ? 'visible' : 'hidden'} order</span><br />
                        <span>Qty {prior.quantity} → {row.quantity} · unit price {Number((orderUnitPrice(prior.platinum, prior.per_trade) ?? 0).toFixed(2))}p → {row.platinum}p</span><br />
                        <span>Lot total {prior.platinum}p → {row.platinum * row.per_trade}p</span><br />
                        <span>Units / trade {prior.per_trade ?? 1} → {row.per_trade} · stays {prior.visible ? 'visible' : 'hidden'}</span>
                      {:else if row.reviewed_order?.state === 'new'}
                        New hidden listing · {row.quantity} × {row.platinum}p · {row.per_trade} units / trade · {row.platinum * row.per_trade}p per lot
                      {:else}Existing order state not confirmed{/if}
                    </td>
                  {/if}
                  <td class="muted">
                    {#if row.sellable < row.owned}
                      {@const bd = ownedBreakdown(row.owned, row.sellable, row.leveled)}
                      {row.owned} owned{#if bd.leveledPart > 0}{' · '}<span class="leveled-note" title={LEVELED_NOTE_TITLE}>{bd.leveledPart} leveled</span>{/if}{#if bd.keptPart > 0}{' · '}<span class="kept-note" title={keptNoteTitle(bd.keptPart)}>{bd.keptPart} kept</span>{/if}
                    {:else}
                      {row.owned} owned
                    {/if}
                  </td>
                  <td>
                    <input
                      type="number"
                      min="5"
                      max={MAX_PLATINUM}
                      bind:value={controller.plan[i].platinum}
                      disabled={!row.include}
                      class:off={row.include && controller.priceOff(row)}
                      title={row.include && controller.priceOff(row) ? `More than 30% off the 48h average (${row.avg.toFixed(0)}p) - double-check before sending` : undefined}
                    />
                  </td>
                  <td class="muted">{plat(row.avg)}</td>
                  <td class="live-cell" class:above={v === 'above'} class:belowbid={v === 'below-bid'}>
                    {#if !t}
                      <span class="muted">·</span>
                    {:else if t.error}
                      <span class="muted" title={t.error}>n/a</span>
                    {:else}
                      {#if t.low_sell != null}
                        <button class="linkish" onclick={() => controller.useLive(i)} disabled={!row.include}
                          title={`Online asks: ${t.sells.join(', ')}p - click to price at ${t.low_sell}p`}>{t.low_sell}p</button>
                      {:else}<span class="muted" title="No online sellers right now">no ask</span>{/if}
                      <span class="muted"> / </span>
                      {#if t.top_buy != null}
                        <span title={`Online bids: ${t.buys.join(', ')}p`}>{t.top_buy}p</span>
                      {:else}<span class="muted" title="No online buyers right now">no bid</span>{/if}
                      {#if v === 'above'}<span class="verdict" title="Your price is above the lowest online ask - it won't be the first to sell.">▲</span>{/if}
                      {#if v === 'below-bid'}<span class="verdict" title="A live buyer is bidding more than your price - you'd be leaving plat on the table.">▼</span>{/if}
                    {/if}
                  </td>
                  <td>
                    <input
                      type="number"
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
        <footer>
          <div class="totals">
            <span><strong>{controller.selectedCount}</strong> items</span>
            <span><strong>{plat(controller.totalPlat)}</strong> plat total</span>
            {#if controller.selectedCount > 50}
              <span class="warn">Batch cap is 50 - deselect some.</span>
            {/if}
          </div>
          <div class="actions">
            <button class="btn ghost" onclick={controller.close}>Cancel</button>
            <button class="btn primary" onclick={controller.send} disabled={!controller.canSubmit || controller.validatingSend || controller.marketAccess.mutationsBlocked}>
              Send {controller.selectedCount} listings
            </button>
          </div>
        </footer>
      {:else if controller.phase === 'sending'}
        <p class="lead">
          Checking current orders and posting your listings. Other market activity can add waiting time.
        </p>
<div class="spinner">{controller.cancellationRequested ? 'Finishing the current request; unsent listings will stay saved.' : controller.marketAccess.message ?? 'Sending…'}</div>
        {#if controller.networkError}<p class="ui-notice" data-tone="bad">{controller.networkError}</p>{/if}
        <button class="btn" onclick={controller.stopSending} disabled={controller.cancellationRequested}>Stop after current request</button>
      {:else if controller.phase === 'results'}
        {#if controller.durabilityError}
          <p class="ui-notice" data-tone="bad" role="alert">{controller.durabilityError}</p>
        {/if}
        <p class="lead">
          {controller.durabilityError ? 'Batch results need attention.' : controller.pendingCount > 0 ? 'Batch interrupted. Close this review and use Resume to revalidate the saved items.' : 'Done.'} <span class="ok">{controller.okCount} created</span>
          {#if controller.updatedCount > 0}· <span class="ok">{controller.updatedCount} updated</span>{/if}
          {#if controller.errCount > 0}· <span class="bad">{controller.errCount} failed</span>{/if}
          {#if controller.pendingCount > 0}· <span class="warn">{controller.pendingCount} saved for resume</span>{/if}.
          {#if controller.visibilityDone}
            Listings are <strong>visible</strong> - buyers can see them now.
          {:else}
            New listings start <strong>hidden</strong> - no buyers can see them
            yet. Flip them visible after you've reviewed the prices.
          {/if}
          Updated orders keep their existing visibility and price history.
        </p>
        <div class="scroll">
          <table>
            <thead><tr><th></th><th>Item</th><th>Detail</th></tr></thead>
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
            Visibility toggled. <span class="ok">{controller.visibleOkCount} now visible</span>
            {#if controller.visibleErrCount > 0}· <span class="bad">{controller.visibleErrCount} failed</span>{/if}.
          </p>
        {/if}

        <footer>
          <div></div>
          <div class="actions">
            {#if controller.okCount > 0 && !controller.visibilityDone}
              <button class="btn primary" onclick={controller.makeAllVisible} disabled={controller.visibilityBusy}>
                {controller.visibilityBusy ? 'Making visible…' : `Make ${controller.okCount} visible`}
              </button>
            {/if}
            <button class={controller.visibilityDone ? 'btn primary' : 'btn ghost'} onclick={controller.close}>Done</button>
          </div>
        </footer>
      {:else if controller.phase === 'error'}
        <p class="lead bad">{controller.networkError}</p>
        <footer>
          <div></div>
          <div class="actions">
            <button class="btn ghost" onclick={controller.close}>Cancel</button>
            <button class="btn primary" onclick={() => (controller.phase = 'review')}>Back to review</button>
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
  .lead strong { color: var(--fg); }
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
  .totals strong { color: var(--fg); font-weight: 600; }
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
