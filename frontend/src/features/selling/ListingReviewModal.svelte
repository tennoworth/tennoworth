<script lang="ts">
  import { useDesktopServices } from '../../ui/desktop-context';
  const { desktopLiveTopPrices, desktopTradeSessionState, isDesktopRuntime, listenForTauriEvent } = useDesktopServices();
  import { onDestroy, untrack, tick } from 'svelte';
  import { DesktopCmdError } from '../../contracts/errors';

import { LIVE_TOP_PROGRESS_EVENT, ALLOWANCE_CHANGED_EVENT } from '../../contracts/events';

import { type LiveTop, type DesktopCapabilities } from '../../contracts/desktop';
  
  import type { ItemResult, SessionConstraint, ReviewedOrder, MarketItemEntry } from '../../contracts/data';
  import { validSessionLot } from '../../domain/trade-session';
  import { clearingPrice } from '../../domain/sell-priority';
  import { orderUnitPrice } from '../../domain/order-prices';
  import { MAX_PLATINUM, MIN_PLATINUM, MAX_PLAN_ITEMS } from '../../domain/limits';
  import { humanError } from '../../contracts/errors';
  import { plat, ownedBreakdown, LEVELED_NOTE_TITLE, keptNoteTitle } from '../../ui/format';
  import DialogHeader from '../../ui/DialogHeader.svelte';

  /** Row shape passed in from ResultsTable / App.svelte. */
  import type { ListingCandidate as InputRow } from '../../contracts/listing';

  interface PlanRow {
    components?: Record<string, number>;
    component_limits?: Record<string, number>;
    key: string;
    slug: string;
    subtype: string | null;
    name: string;
    include: boolean;
    platinum: number;
    quantity: number;
    owned: number;
    sellable: number;
    leveled: number;
    rank: number;
    reference_low_sell: number;
    avg: number;
    per_trade: number;
    bulk: boolean;
    session?: SessionConstraint;
    reviewed_order?: ReviewedOrder;
    market?: MarketItemEntry;
  }

  interface ReviewOrderWire {
    id: string; platinum: number; quantity: number; visible: boolean;
    perTrade?: number | null; rank?: number; subtype?: string | null;
    type?: string; item?: { slug?: string };
  }

  interface Props {
    open?: boolean;
    rows: InputRow[];
    /** The app's boot-selected transport - Tauri IPC into wfm-core. */
    transport: DesktopCapabilities;
    /** Desktop only: a listing call came back `needs_login` / `needs_unlock`.
     *  The app opens the matching auth dialog on top of this modal; the user
     *  resends after authenticating. */
    onauthrequired?: (code: 'needs_login' | 'needs_unlock') => void;
    onclose?: () => void;
  }
  let { open = $bindable(false), rows, transport, onauthrequired, onclose }: Props = $props();

  let plan = $state<PlanRow[]>([]);
  type Phase = 'review' | 'sending' | 'results' | 'error';
  let phase = $state<Phase>('review');
  let serverResults = $state<ItemResult[]>([]);
  let networkError = $state<string | null>(null);

  function initialPlanFor(rows: InputRow[]): PlanRow[] {
    return rows.map((r) => {
      // Prefill from the clamped clearing price, not raw low_sell - the raw
      // ask inherits every troll listing (a lone 100p ask on a 10p item, or
      // a 1p undercut on an undercut day). Falls back for older callers.
      const target =
        (r.clearing_price ?? 0) > 0 ? Math.ceil(r.clearing_price as number)
        : r.low_sell > 0 ? r.low_sell
        : Math.round(r.avg_price);
      // Cap using sellable (owned minus the "Keep copies" reserve), not raw
      // owned - this is the last line of defense against listing a copy the
      // user asked to hold back. Falls back to owned for callers that
      // predate the reserve field.
      const sellable = r.sellable ?? r.owned;
      return {
        key: r.key ?? r.slug,
        components: r.components,
        component_limits: r.component_limits ? { ...r.component_limits } : undefined,
        slug: r.slug,
        subtype: r.subtype ?? null,
        name: r.name,
        include: true,
        platinum: Math.max(5, target),
        quantity: r.proposed_quantity ?? 1,
        per_trade: r.per_trade ?? 1,
        bulk: r.bulk ?? false,
        session: r.session,
        market: r.market,
        owned: r.owned,
        sellable,
        leveled: r.leveled ?? 0,
        // Rank 0 = unranked, the tier dupe stacks actually are. Editable so
        // a leveled copy can be listed at its real rank; the app only
        // sends rank for items WFM ranks (mods/arcanes), so a non-zero rank
        // on a rankless item is ignored server-side, not an error.
        rank: 0,
        reference_low_sell: r.low_sell || 0,
        avg: r.avg_price,
      };
    });
  }

  // Prefill sanity flag: a suggested price far off the 48h average deserves
  // a second look before it goes out in a 50-item batch.
  function priceOff(r: PlanRow): boolean {
    return r.avg > 0 && (r.platinum > r.avg * 1.3 || r.platinum < r.avg * 0.7);
  }

  // Re-initialize when the modal OPENS - and only then.
  //
  // `rows` is read through untrack() deliberately. The caller passes
  // `reviewRowsOverride ?? listableRows.slice(0, 50)`, and that .slice() mints
  // a fresh array identity every time the `listableRows` derived recomputes.
  // Tracking it meant any background recompute while the modal was open
  // re-ran this init and threw away the user's in-flight price and quantity
  // edits, mid-review, on a batch of up to 50 listings. The rows to review are
  // whatever they were when the modal opened; nothing here wants live updates.
  $effect(() => {
    if (open) {
      reviewEpoch++;
      plan = initialPlanFor(untrack(() => rows) ?? []);
      phase = 'review';
      serverResults = [];
      networkError = null;
      ordersReady = false;
      sessionRemaining = null;
      sessionProblem = null;
      untrack(() => { if (plan.some(r => r.session)) void refreshReviewOrders(false); });
    }
  });

  let selectedCount = $derived(plan.filter((r) => r.include).length);
  let reviewEpoch = 0;
  let hasSession = $derived(plan.some(r => r.session));
  let ordersReady = $state(false);
  let ordersBusy = $state(false);
  let sessionRemaining = $state<number | null>(null);
  let sessionProblem = $state<string | null>(null);
  let estimatedTrades = $derived(plan.filter(r => r.include).every(r => validSessionLot(r.quantity, r.per_trade, r.bulk))
    ? plan.filter(r => r.include).reduce((sum, r) => sum + r.quantity / r.per_trade, 0) : null);

  async function refreshSession(): Promise<boolean> {
    if (!hasSession) return true;
    const epoch = reviewEpoch;
    try {
      const state = await desktopTradeSessionState();
      if (!open || epoch !== reviewEpoch) return false;
      const context = plan[0]?.session;
      sessionRemaining = state.allowance.remaining;
      for (const row of plan) {
        row.sellable = Math.min(row.sellable, state.quantities[row.slug] ?? 0);
        if (row.component_limits) for (const slug of Object.keys(row.component_limits)) {
          row.component_limits[slug] = Math.min(row.component_limits[slug], state.quantities[slug] ?? 0);
        }
        if (state.supported_slugs && !state.supported_slugs.includes(row.slug)) row.sellable = 0;
        row.bulk = state.bulk_slugs.includes(row.slug);
      }
      sessionProblem = context && state.allowance.snapshot_id === context.snapshot_id && state.allowance.utc_day === context.utc_day
        ? null : 'The inventory or allowance day changed. Close review and prepare a new batch.';
      await tick();
      return sessionProblem == null;
    } catch (e) { if (open && epoch === reviewEpoch) sessionProblem = humanError(e); return false; }
  }

  async function refreshReviewOrders(confirm: boolean): Promise<boolean> {
    if (!hasSession) return true;
    const epoch = reviewEpoch;
    ordersBusy = true;
    try {
      if (!await refreshSession()) return false;
      const body = await transport.fetchOrders() as { data?: { sell?: ReviewOrderWire[] } | ReviewOrderWire[]; sell?: ReviewOrderWire[] };
      if (!open || epoch !== reviewEpoch) return false;
      const data = body?.data ?? body;
      const orders = Array.isArray(data) ? data.filter(o => o.type === 'sell') : data?.sell;
      if (!Array.isArray(orders)) throw new Error('Could not read existing orders. Retry before submitting.');
      let changed = false;
      for (const row of plan) {
        const matches = orders.filter(o => o.item?.slug === row.slug && (o.rank ?? 0) === row.rank && (o.subtype ?? null) === row.subtype);
        if (matches.length > 1) throw new Error(`${row.name} has ambiguous existing orders. Resolve them in My orders first.`);
        const prior = matches[0];
        if (prior && (typeof prior.id !== 'string' || typeof prior.visible !== 'boolean' || !Number.isSafeInteger(prior.platinum) || !Number.isSafeInteger(prior.quantity)
          || (prior.perTrade != null && (!Number.isSafeInteger(prior.perTrade) || prior.perTrade < 1 || prior.perTrade > 6)))) {
          throw new Error(`Existing order details are incomplete for ${row.name}.`);
        }
        const next: ReviewedOrder = prior ? { state: 'existing', id: prior.id, platinum: prior.platinum,
          quantity: prior.quantity, per_trade: prior.perTrade ?? null, visible: prior.visible } : { state: 'new' };
        if (JSON.stringify(row.reviewed_order) !== JSON.stringify(next)) changed = true;
        row.reviewed_order = next;
      }
      ordersReady = true;
      if (confirm && changed) {
        networkError = 'Existing orders changed. Review the updated before/after details, then confirm again. Your edits were kept.';
        return false;
      }
      return true;
    } catch (e) {
      if (!open || epoch !== reviewEpoch) return false;
      ordersReady = false;
      if (!handleAuthCode(e)) networkError = humanError(e);
      return false;
    } finally { if (epoch === reviewEpoch) ordersBusy = false; }
  }

  $effect(() => {
    if (!open || !hasSession) return;
    return listenForTauriEvent(ALLOWANCE_CHANGED_EVENT, () => {
      if (phase === 'review') void refreshSession();
    });
  });

  // ---- Live prices (desktop only) ----
  // The prefill comes from the 2-hourly snapshot. One click asks WFM for the
  // ≤5 best ONLINE asks/bids for each selected row's exact tier (rank /
  // relic refinement) - the price you'd actually be competing with right
  // now. Paced at WFM's 3 req/s, so a big batch shows a counter.
  const canLive = isDesktopRuntime();
  type LiveState = 'idle' | 'running' | 'done' | 'error';
  let liveState = $state<LiveState>('idle');
  let liveProgress = $state({ done: 0, total: 0 });
  let liveError = $state<string | null>(null);
  let live = $state<Map<string, LiveTop>>(new Map());
  let liveListenerArmed = false;
  let unlistenLiveProgress = () => {};

  onDestroy(() => unlistenLiveProgress());

  function liveKey(slug: string, rank: number, subtype: string | null): string {
    return `${slug}|${rank}|${subtype ?? ''}`;
  }
  function liveFor(row: PlanRow): LiveTop | undefined {
    return live.get(liveKey(row.slug, row.rank, row.subtype));
  }

  async function checkLivePrices(): Promise<void> {
    const targets = plan.filter((r) => r.include);
    if (targets.length === 0) return;
    if (!liveListenerArmed) {
      liveListenerArmed = true;
      unlistenLiveProgress = listenForTauriEvent<{ done: number; total: number }>(LIVE_TOP_PROGRESS_EVENT, (p) => {
        liveProgress = p;
      });
    }
    liveState = 'running';
    liveError = null;
    liveProgress = { done: 0, total: targets.length };
    try {
      const res = await desktopLiveTopPrices(
        targets.map((r) => ({ slug: r.slug, rank: r.rank, subtype: r.subtype })),
      );
      const next = new Map(live);
      for (const t of res) next.set(liveKey(t.slug, t.rank ?? 0, t.subtype ?? null), t);
      live = next;
      liveState = 'done';
    } catch (e) {
      liveState = 'error';
      liveError = e instanceof DesktopCmdError ? e.message : humanError(e);
    }
  }

  /** Set the row's price to the live lowest online ask (match, don't undercut). */
  function useLive(i: number): void {
    const t = liveFor(plan[i]);
    if (t?.low_sell != null) {
      const row = plan[i];
      row.platinum = Math.max(MIN_PLATINUM, Math.ceil(row.session && row.market
        ? clearingPrice({ ...row.market, low_sell: t.low_sell }) : t.low_sell));
    }
  }
  function useLiveAll(): void {
    plan.forEach((_, i) => { if (plan[i].include) useLive(i); });
  }
  /** How the row's price sits against the live book: over the lowest ask
   *  (won't sell first), under the top bid (leaving plat on the table), or ok. */
  function liveVerdict(row: PlanRow): 'above' | 'below-bid' | 'ok' | null {
    const t = liveFor(row);
    if (!t || t.error) return null;
    if (t.low_sell != null && row.platinum > t.low_sell) return 'above';
    if (t.top_buy != null && row.platinum < t.top_buy) return 'below-bid';
    return 'ok';
  }
  let totalPlat = $derived(
    plan
      .filter((r) => r.include)
      .reduce((s, r) => s + r.platinum * r.quantity, 0)
  );
  let allocationProblem = $derived.by(() => {
    const used = new Map<string, number>();
    const limits = new Map<string, number>();
    for (const row of plan.filter(row => row.include && row.component_limits)) {
      for (const [slug, count] of Object.entries(row.components ?? { [row.slug]: 1 })) {
        used.set(slug, (used.get(slug) ?? 0) + count * row.quantity);
        limits.set(slug, Math.min(limits.get(slug) ?? Infinity, row.component_limits?.[slug] ?? 0));
      }
    }
    return [...used].some(([slug, count]) => !Number.isSafeInteger(count) || count > (limits.get(slug) ?? 0))
      ? 'These edited quantities reuse components or exceed their available copies. Reduce a set or part quantity.' : null;
  });
  let canSubmit = $derived(
    !allocationProblem && selectedCount > 0 && selectedCount <= MAX_PLAN_ITEMS && plan.every(
      (r) => !r.include || (Number.isSafeInteger(r.platinum) && r.platinum >= MIN_PLATINUM && r.platinum <= MAX_PLATINUM && Number.isSafeInteger(r.quantity) && r.quantity >= 1 && r.quantity <= r.sellable
        && (!r.session || (validSessionLot(r.quantity, r.per_trade, r.bulk) && r.platinum * r.per_trade <= MAX_PLATINUM)))
    ) && (!hasSession || (ordersReady && !ordersBusy && !sessionProblem && estimatedTrades != null && estimatedTrades <= (sessionRemaining ?? 0) && estimatedTrades <= (plan[0]?.session?.budget ?? 0)))
  );

  function close(): void {
    open = false;
    onclose?.();
  }

  /** Desktop lock-state rejection → hand off to the auth dialogs and return to
   *  review so Send is one click away once the session unlocks. */
  function handleAuthCode(e: unknown): boolean {
    if (e instanceof DesktopCmdError && (e.code === 'needs_login' || e.code === 'needs_unlock')) {
      phase = 'review';
      onauthrequired?.(e.code);
      return true;
    }
    return false;
  }

  async function send(): Promise<void> {
    if (hasSession && !await refreshReviewOrders(true)) return;
    await tick();
    if (!canSubmit) { networkError = 'Review quantities, units per trade, prices, and the remaining allowance before submitting.'; return; }
    phase = 'sending';
    networkError = null;
    const items = plan
      .filter((r) => r.include)
      .map((r) => ({
        slug: r.slug,
        platinum: r.platinum,
        quantity: r.quantity,
        per_trade: r.session ? r.per_trade : undefined,
        session: r.session,
        reviewed_order: r.session ? r.reviewed_order : undefined,
        order_type: 'sell' as const,
        visible: false,
        rank: r.rank > 0 ? r.rank : undefined,
        subtype: r.subtype || undefined,
        reference_low_sell: r.reference_low_sell || undefined,
      }));
    try {
      const resp = await transport.submitPlan(items);
      serverResults = resp.results || [];
      phase = 'results';
    } catch (e) {
      if (handleAuthCode(e)) return;
      networkError = humanError(e);
      phase = 'error';
    }
  }

  let updatedCount = $derived(
    serverResults.filter((r) => r.status === 'ok' && r.action === 'updated').length,
  );
  let okCount = $derived(
    serverResults.filter((r) => r.status === 'ok').length - updatedCount,
  );
  let errCount = $derived(serverResults.filter((r) => r.status !== 'ok').length);

  // The results table only carries slugs; map back to human names so the
  // "what did I just list" review isn't a wall of snake_case.
  let planNameBySlug = $derived(
    new Map(plan.map((r) => [r.slug, r.name])),
  );

  let visibilityBusy = $state(false);
  let visibilityDone = $state(false);
  let visibilityResults = $state<ItemResult[]>([]);

  function setAll(include: boolean): void {
    for (let i = 0; i < plan.length; i++) plan[i].include = include;
  }

  async function makeAllVisible(): Promise<void> {
    // Only freshly-created orders: updated ones keep the visibility the user
    // chose on WFM (matches the "N created" count on the button).
    const ids = serverResults
      .filter((r) => r.status === 'ok' && r.action !== 'updated' && r.order_id)
      .map((r) => r.order_id as string);
    if (ids.length === 0) return;
    visibilityBusy = true;
    try {
      const resp = await transport.bulkVisibility(ids, true);
      visibilityResults = resp?.results || [];
      visibilityDone = true;
    } catch (e) {
      // A lock-state rejection here (desktop logout between send and toggle)
      // must not dump the user to the error phase and lose the results table.
      if (e instanceof DesktopCmdError && (e.code === 'needs_login' || e.code === 'needs_unlock')) {
        onauthrequired?.(e.code);
      } else {
        networkError = humanError(e);
        phase = 'error';
      }
    } finally {
      visibilityBusy = false;
    }
  }

  let visibleOkCount = $derived(visibilityResults.filter((r) => r.status === 'ok').length);
  let visibleErrCount = $derived(visibilityResults.filter((r) => r.status !== 'ok').length);

  function reviewFocus(node: HTMLElement) {
    const previous = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    let mounted = true;
    const controls = () => [...node.querySelectorAll<HTMLElement>('button, input, select, textarea, a[href], [tabindex]')]
      .filter(el => !el.matches(':disabled, [tabindex="-1"]') && el.getClientRects().length > 0);
    queueMicrotask(() => { if (mounted) controls()[0]?.focus(); });
    const onKey = (event: KeyboardEvent) => {
      if (event.defaultPrevented || document.querySelector('dialog[open]')) return;
      if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); close(); }
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
    <div class="modal" class:session={hasSession}>
      <DialogHeader titleId="rm-title" title="List on warframe.market" onclose={close} />

      {#if phase === 'review'}
        <p class="lead">
          {#if hasSession}
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

        {#if hasSession}
          {#if allocationProblem}<p class="ui-notice" data-tone="warn" role="alert">{allocationProblem}</p>{/if}
          <p class="ui-notice" data-tone={canSubmit ? 'good' : 'warn'}>
            {estimatedTrades ?? 'Invalid quantity / lot'} estimated trades / {plan[0]?.session?.budget} budget · {sessionRemaining ?? 'unknown'} remaining.
            Quantities must divide evenly by units per trade. Posting does not spend the game allowance.
          </p>
          {#if sessionProblem}<p class="ui-notice" data-tone="warn">{sessionProblem}</p>{/if}
          {#if plan.some(r => r.include && r.quantity > r.sellable)}
            <p class="ui-notice" data-tone="warn">A selected quantity exceeds the confirmed sellable count. Your edits are retained; reduce the quantity or scan again.</p>
          {/if}
          {#if networkError}<p class="ui-notice" data-tone="warn" role="alert">{networkError}</p>{/if}
          <button class="btn ghost" onclick={() => refreshReviewOrders(false)} disabled={ordersBusy}>
            {ordersBusy ? 'Checking existing orders…' : 'Refresh existing orders'}
          </button>
        {/if}

        <div class="bulkrow">
          <button class="btn ghost" onclick={() => setAll(true)}>Select all</button>
          <button class="btn ghost" onclick={() => setAll(false)}>Deselect all</button>
          {#if canLive}
            <span class="spacer"></span>
            <button
              class="btn ghost live-btn"
              onclick={checkLivePrices}
              disabled={liveState === 'running' || selectedCount === 0}
              title="Ask warframe.market for the ≤5 best online asks and bids for each selected row's exact rank / refinement, right now. Paced to WFM's rate limit (~3 items per second)."
            >
              {#if liveState === 'running'}
                Checking live prices… {liveProgress.done}/{liveProgress.total}
              {:else if liveState === 'done'}
                Re-check live prices
              {:else}
                Check live prices
              {/if}
            </button>
            {#if liveState === 'done' && live.size > 0}
              <button class="btn ghost" onclick={useLiveAll} title="Set every selected row's price to its live lowest online ask (match it - no undercutting).">Match lowest asks</button>
            {/if}
            {#if liveState === 'error' && liveError}
              <span class="live-err">{liveError}</span>
            {/if}
          {/if}
        </div>

        <div class="scroll">
          <table>
            <thead>
              <tr>
                <th></th>
                <th>Item</th>
                <th>Qty</th>
                {#if hasSession}<th>Units / trade</th><th>Existing → proposed</th>{/if}
                <th>Owned</th>
                <th>Price (p)</th>
                <th>Avg</th>
                {#if canLive}<th title="Live lowest online ask / highest online bid for this exact rank or refinement (after “Check live prices”). Click a value to use it.">Live ask / bid</th>{/if}
                <th title="Mod/arcane rank of the copies you're listing. 0 = unranked (dupe stacks). Ignored for items WFM doesn't rank.">Rank</th>
                <th>Subtotal</th>
              </tr>
            </thead>
            <tbody>
              {#each plan as row, i (row.key)}
                <tr class:dim={!row.include}>
                  <td><input type="checkbox" bind:checked={plan[i].include} /></td>
                  <td>{row.name}</td>
                  <td>
                    <input
                      type="number"
                      min="1"
                      max={row.sellable}
                      bind:value={plan[i].quantity}
                      disabled={!row.include}
                    />
                  </td>
                  {#if hasSession}
                    <td><input type="number" aria-label={`Units per trade for ${row.name}`} min="1" max={row.bulk ? 6 : 1}
                      step="1" bind:value={plan[i].per_trade} disabled={!row.include} /></td>
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
                      bind:value={plan[i].platinum}
                      disabled={!row.include}
                      class:off={row.include && priceOff(row)}
                      title={row.include && priceOff(row) ? `More than 30% off the 48h average (${row.avg.toFixed(0)}p) - double-check before sending` : undefined}
                    />
                  </td>
                  <td class="muted">{plat(row.avg)}</td>
                  {#if canLive}
                    {@const t = liveFor(row)}
                    {@const v = liveVerdict(row)}
                    <td class="live-cell" class:above={v === 'above'} class:belowbid={v === 'below-bid'}>
                      {#if !t}
                        <span class="muted">·</span>
                      {:else if t.error}
                        <span class="muted" title={t.error}>n/a</span>
                      {:else}
                        {#if t.low_sell != null}
                          <button class="linkish" onclick={() => useLive(i)} disabled={!row.include}
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
                  {/if}
                  <td>
                    <input
                      type="number"
                      min="0"
                      max="10"
                      class="rank"
                      bind:value={plan[i].rank}
                      disabled={!row.include || !!row.session}
                    />
                  </td>
                  <td class="right">{plat(row.platinum * row.quantity)}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>

        <footer>
          <div class="totals">
            <span><strong>{selectedCount}</strong> items</span>
            <span><strong>{plat(totalPlat)}</strong> plat total</span>
            {#if selectedCount > 50}
              <span class="warn">Batch cap is 50 - deselect some.</span>
            {/if}
          </div>
          <div class="actions">
            <button class="btn ghost" onclick={close}>Cancel</button>
            <button class="btn primary" onclick={send} disabled={!canSubmit}>
              Send {selectedCount} listings
            </button>
          </div>
        </footer>
      {:else if phase === 'sending'}
        <p class="lead">
          Posting to warframe.market. ~3 listings/second -
          this will take ~{Math.ceil((selectedCount * 0.35) + 1)} s.
        </p>
        <div class="spinner">Sending…</div>
      {:else if phase === 'results'}
        <p class="lead">
          Done. <span class="ok">{okCount} created</span>
          {#if updatedCount > 0}· <span class="ok">{updatedCount} updated</span>{/if}
          {#if errCount > 0}· <span class="bad">{errCount} failed</span>{/if}.
          {#if visibilityDone}
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
              {#each serverResults as r, i (i)}
                <tr>
                  <td class:ok={r.status === 'ok'} class:bad={r.status !== 'ok'}>
                    {r.status === 'ok' ? '✓' : '✗'}
                  </td>
                  <td>
                    <span class="item-name">{planNameBySlug.get(r.slug) ?? r.slug}</span>
                    <span class="item-slug">{r.slug}</span>
                  </td>
                  <td class="muted">{r.message ?? r.order_id ?? ''}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
        {#if visibilityDone}
          <p class="lead">
            Visibility toggled. <span class="ok">{visibleOkCount} now visible</span>
            {#if visibleErrCount > 0}· <span class="bad">{visibleErrCount} failed</span>{/if}.
          </p>
        {/if}

        <footer>
          <div></div>
          <div class="actions">
            {#if okCount > 0 && !visibilityDone}
              <button class="btn primary" onclick={makeAllVisible} disabled={visibilityBusy}>
                {visibilityBusy ? 'Making visible…' : `Make ${okCount} visible`}
              </button>
            {/if}
            <button class={visibilityDone ? 'btn primary' : 'btn ghost'} onclick={close}>Done</button>
          </div>
        </footer>
      {:else if phase === 'error'}
        <p class="lead bad">{networkError}</p>
        <footer>
          <div></div>
          <div class="actions">
            <button class="btn ghost" onclick={close}>Cancel</button>
            <button class="btn primary" onclick={() => (phase = 'review')}>Back to review</button>
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
