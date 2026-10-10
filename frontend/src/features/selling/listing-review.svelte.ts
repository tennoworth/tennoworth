import { untrack, tick } from 'svelte';
  import { WfmAccessController } from './controller.svelte';
  import { LiveTopController } from './live-top.svelte';

  import { DesktopCmdError } from '../../contracts/errors';

import { ALLOWANCE_CHANGED_EVENT } from '../../contracts/events';

import { type LiveTop, type DesktopCapabilities } from '../../contracts/desktop';
  
  import type { ItemResult, SessionConstraint, ReviewedOrder, MarketItemEntry } from '../../contracts/data';
  import { validSessionLot } from '../../domain/trade-session';
  import { clearingPrice } from '../../domain/sell-priority';
  import { MAX_PLATINUM, MIN_PLATINUM, MAX_PLAN_ITEMS, MAX_PER_TRADE } from '../../domain/limits';
  import { humanError } from '../../contracts/errors';

  /** Row shape passed in from ResultsTable / App.svelte. */
  import type { ListingCandidate as InputRow } from '../../contracts/listing';

  interface PlanRow {
    inventory_snapshot_id?: number;
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

  import type { OwnOrder } from '../../contracts/generated/desktop';

import type { DesktopServices } from '../../contracts/services';
  export interface ListingReviewInput {
    open: boolean;
    rows: InputRow[];
    /** The app's boot-selected transport - Tauri IPC into wfm-core. */
    transport: DesktopCapabilities;
    /** Desktop only: a listing call came back `needs_login` / `needs_unlock`.
     *  The app opens the matching auth dialog on top of this modal; the user
     *  resends after authenticating. */
    onauthrequired?: (code: 'needs_login' | 'needs_unlock') => void;
    onclose?: () => void;
    /** Wraps the send so the caller can track it as in-flight and re-read the
     *  saved batch once it settles. Passed in rather than known here: this
     *  component sends, it does not own the batch's lifecycle. */
    sendThrough: <T>(send: () => Promise<T>) => Promise<T>;
    /** Confirmed send results, so the row that was listed can show it. */
    onsent?: (sent: { slug: string; platinum: number; quantity: number }[], results: ItemResult[]) => void;
    /** Confirmed visibility changes from "Make N visible". */
    onvisible?: (results: ItemResult[]) => void;
    listingBlockReason?: string | null;
    onrecheck?: () => void;
    listingActionLabel?: string;
    currentSnapshotId?: number | null;
  }

export function createListingReview(input: ListingReviewInput, services: Pick<DesktopServices, 'desktopAccessStatus' | 'desktopLiveTopPrices' | 'desktopTradeSessionState' | 'listenForTauriEvent'>) {
  const { desktopTradeSessionState, listenForTauriEvent } = services;
  const liveTop = new LiveTopController(services);
  const marketAccess = new WfmAccessController(services);
  function start() { return marketAccess.start(); }
  let plan = $state<PlanRow[]>([]);
  let reviewBlockReason = $derived(input.listingBlockReason ?? (input.currentSnapshotId !== undefined && plan.some(row => row.inventory_snapshot_id !== input.currentSnapshotId)
    ? 'Inventory changed. Close review and prepare a new batch; your current edits remain here until you close.' : null));
  type Phase = 'review' | 'sending' | 'results' | 'error';
  let phase = $state<Phase>('review');
  let validatingSend = $state(false);
  let cancellationRequested = $state(false);
  let serverResults = $state<ItemResult[]>([]);
  let networkError = $state<string | null>(null);
  let durabilityError = $state<string | null>(null);

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
        inventory_snapshot_id: r.inventory_snapshot_id,
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
    if (input.open) {
      reviewEpoch++;
      plan = initialPlanFor(untrack(() => input.rows) ?? []);
      phase = 'review';
      serverResults = [];
      networkError = null;
      durabilityError = null;
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
      if (!input.open || epoch !== reviewEpoch) return false;
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
    } catch (e) { if (input.open && epoch === reviewEpoch) sessionProblem = humanError(e); return false; }
  }

  async function refreshReviewOrders(confirm: boolean): Promise<boolean> {
    if (!hasSession) return true;
    const epoch = reviewEpoch;
    ordersBusy = true;
    try {
      if (!await refreshSession()) return false;
      const orders: OwnOrder[] = await input.transport.fetchOrders();
      if (!input.open || epoch !== reviewEpoch) return false;
      let changed = false;
      for (const row of plan) {
        const matches = orders.filter(o => o.side === 'sell' && o.slug === row.slug && (o.rank ?? 0) === row.rank && (o.subtype ?? null) === row.subtype);
        if (matches.length > 1) throw new Error(`${row.name} has ambiguous existing orders. Resolve them in My orders first.`);
        const prior = matches[0];
        if (prior && (typeof prior.id !== 'string' || typeof prior.visible !== 'boolean' || !Number.isSafeInteger(prior.platinum) || !Number.isSafeInteger(prior.quantity)
          || (prior.per_trade != null && (!Number.isSafeInteger(prior.per_trade) || prior.per_trade < 1 || prior.per_trade > MAX_PER_TRADE)))) {
          throw new Error(`Existing order details are incomplete for ${row.name}.`);
        }
        const next: ReviewedOrder = prior ? { state: 'existing', id: prior.id, platinum: prior.platinum,
          quantity: prior.quantity, per_trade: prior.per_trade, visible: prior.visible as boolean } : { state: 'new' };
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
      if (!input.open || epoch !== reviewEpoch) return false;
      ordersReady = false;
      if (!handleAuthCode(e)) networkError = humanError(e);
      return false;
    } finally { if (epoch === reviewEpoch) ordersBusy = false; }
  }

  $effect(() => {
    if (!input.open || !hasSession) return;
    return listenForTauriEvent(ALLOWANCE_CHANGED_EVENT, () => {
      if (phase === 'review') void refreshSession();
    });
  });

  // ---- Live prices (desktop only) ----
  // The prefill comes from the 2-hourly snapshot. One click asks WFM for the
  // ≤5 best ONLINE asks/bids for each selected row's exact tier (rank /
  // relic refinement) - the price you'd actually be competing with right
  // now. A shared request budget makes batch progress useful.
  function dispose() { liveTop.dispose(); }

  function liveFor(row: PlanRow): LiveTop | undefined {
    return liveTop.get(row);
  }

  async function checkLivePrices(): Promise<void> {
    await liveTop.check(plan.filter((r) => r.include).map((r) => ({ slug: r.slug, rank: r.rank, subtype: r.subtype })));
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
  let canSubmit = $derived(!reviewBlockReason &&
    !allocationProblem && selectedCount > 0 && selectedCount <= MAX_PLAN_ITEMS && plan.every(
      (r) => !r.include || (Number.isSafeInteger(r.platinum) && r.platinum >= MIN_PLATINUM && r.platinum <= MAX_PLATINUM && Number.isSafeInteger(r.quantity) && r.quantity >= 1 && r.quantity <= r.sellable
        && (!r.session || (validSessionLot(r.quantity, r.per_trade, r.bulk) && r.platinum * r.per_trade <= MAX_PLATINUM)))
    ) && (!hasSession || (ordersReady && !ordersBusy && !sessionProblem && estimatedTrades != null && estimatedTrades <= (sessionRemaining ?? 0) && estimatedTrades <= (plan[0]?.session?.budget ?? 0)))
  );

  function close(): void {
    input.open = false;
    input.onclose?.();
  }

  /** Desktop lock-state rejection → hand off to the auth dialogs and return to
   *  review so Send is one click away once the session unlocks. */
  function handleAuthCode(e: unknown): boolean {
    if (e instanceof DesktopCmdError && (e.code === 'needs_login' || e.code === 'needs_unlock')) {
      phase = 'review';
      input.onauthrequired?.(e.code);
      return true;
    }
    return false;
  }

  async function send(): Promise<void> {
    if (phase === 'sending' || validatingSend || marketAccess.mutationsBlocked || reviewBlockReason) return;
    validatingSend = true;
    try { if (hasSession && !await refreshReviewOrders(true)) return; } finally { validatingSend = false; }
    await tick();
    if (!canSubmit) { networkError = 'Review quantities, units per trade, prices, and the remaining allowance before submitting.'; return; }
    phase = 'sending';
    cancellationRequested = false;
    networkError = null;
    const items = plan
      .filter((r) => r.include)
      .map((r) => ({
        inventory_snapshot_id: r.inventory_snapshot_id,
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
      const resp = await input.sendThrough(() => input.transport.submitPlan(items));
      serverResults = resp.results || [];
      durabilityError = resp.durability_error ?? null;
      phase = 'results';
      input.onsent?.(items.map(({ slug, platinum, quantity }) => ({ slug, platinum, quantity })), serverResults);
    } catch (e) {
      if (handleAuthCode(e)) return;
      networkError = humanError(e);
      phase = 'error';
    }
  }

  async function stopSending() {
    if (cancellationRequested) return;
    cancellationRequested = true;
    try { await input.transport.cancelPlan(); } catch (error) { cancellationRequested = false; networkError = humanError(error); }
  }

  let updatedCount = $derived(
    serverResults.filter((r) => r.status === 'ok' && r.action === 'updated').length,
  );
  let okCount = $derived(
    serverResults.filter((r) => r.status === 'ok').length - updatedCount,
  );
  let pendingCount = $derived(serverResults.filter(r => r.status === 'pending' || r.status === 'uncertain_mutation').length);
  let errCount = $derived(serverResults.filter(r => r.status !== 'ok' && r.status !== 'pending' && r.status !== 'uncertain_mutation').length);

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
      const resp = await input.transport.bulkVisibility(ids, true);
      visibilityResults = resp?.results || [];
      input.onvisible?.(visibilityResults);
      // "Buyers can see them now" is a claim that WFM accepted the toggle, so it
      // takes at least one confirmed row: the call returning is not the change
      // landing. The counts beside it already report how many failed.
      visibilityDone = visibilityResults.some((r) => r.status === 'ok');
    } catch (e) {
      // A lock-state rejection here (desktop logout between send and toggle)
      // must not dump the user to the error phase and lose the results table.
      if (e instanceof DesktopCmdError && (e.code === 'needs_login' || e.code === 'needs_unlock')) {
        input.onauthrequired?.(e.code);
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

  return {
    priceOff,
    refreshSession,
    refreshReviewOrders,
    dispose,
    liveFor,
    checkLivePrices,
    useLive,
    useLiveAll,
    liveVerdict,
    close,
    send,
    stopSending,
    setAll,
    makeAllVisible,
    start,
    get plan() { return plan; },
    get reviewBlockReason() { return reviewBlockReason; },
    get phase() { return phase; },
    set phase(value: typeof phase) { phase = value; },
    get validatingSend() { return validatingSend; },
    get cancellationRequested() { return cancellationRequested; },
    get serverResults() { return serverResults; },
    get networkError() { return networkError; },
    get durabilityError() { return durabilityError; },
    get selectedCount() { return selectedCount; },
    get hasSession() { return hasSession; },
    get ordersReady() { return ordersReady; },
    get ordersBusy() { return ordersBusy; },
    get sessionRemaining() { return sessionRemaining; },
    get sessionProblem() { return sessionProblem; },
    get estimatedTrades() { return estimatedTrades; },
    get totalPlat() { return totalPlat; },
    get allocationProblem() { return allocationProblem; },
    get canSubmit() { return canSubmit; },
    get updatedCount() { return updatedCount; },
    get okCount() { return okCount; },
    get pendingCount() { return pendingCount; },
    get errCount() { return errCount; },
    get planNameBySlug() { return planNameBySlug; },
    get visibilityBusy() { return visibilityBusy; },
    get visibilityDone() { return visibilityDone; },
    get visibilityResults() { return visibilityResults; },
    get visibleOkCount() { return visibleOkCount; },
    get visibleErrCount() { return visibleErrCount; },
    get liveTop() { return liveTop; },
    get marketAccess() { return marketAccess; },
  };
}
