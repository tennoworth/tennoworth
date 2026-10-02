import { DesktopCmdError } from '../../contracts/errors';

  import { LiveTopController } from '../selling/live-top.svelte';

import { type LiveTop, type DesktopCapabilities } from '../../contracts/desktop';
  
  import { MAX_PLATINUM, MIN_PLATINUM } from '../../domain/limits';
  import { humanError } from '../../contracts/errors';
  import { selectDrifted, type DriftRow } from '../../domain/order-drift';
  import { assessListings, summarize, ownedEvidence, type HealthIssue } from '../../domain/listing-health';
  import type { Market } from '../../contracts/data';
  import { orderUnitPrice, orderLotPrice, cachedUnitMarket } from '../../domain/order-prices';

  import type { OwnOrder } from '../../contracts/generated/desktop';

  interface ToastMsg {
    id: number;
    kind: 'error' | 'success';
    text: string;
  }

import type { DesktopServices } from '../../contracts/services';
  export interface OrdersInput {
    transport: DesktopCapabilities;
    /** Price reference for the drift check. Null on a snapshot-less load -
     *  the drift section simply does not render. */
    market?: Market | null;
    /** Bumped by the parent when the WFM session unlocks - re-fetches so a
     *  fetch gated on needs_login/needs_unlock retries automatically. */
    sessionEpoch?: number;
    /** Desktop lock-state rejection → parent raises the auth dialog (which
     *  tries the OS-keyring silent unlock before showing the passphrase). */
    onauthrequired?: (code: 'needs_login' | 'needs_unlock') => void;
    /** Tradeable copies owned per the latest scan, keyed by `ownedKey(slug,
     *  subtype)`. Null when there is no scan - the quantity checks stay off. */
    ownedQty?: Map<string, number> | null;
    /** Live-orders summary for the shell strip / Sell summary cells: fired
     *  once orders are loaded (and whenever the count or the health issues
     *  change); null while nothing is loaded so those cells stay hidden. */
    onsummary?: (s: { live: number; issues: number } | null) => void;
  }

export function createOrdersController(input: OrdersInput, services: Pick<DesktopServices, 'desktopLiveTopPrices' | 'listenForTauriEvent'>) {
  const liveTop = new LiveTopController(services);
  type Phase = 'idle' | 'loading' | 'locked' | 'done' | 'error';
  let phase = $state<Phase>('idle');
  let error = $state<string | null>(null);
  let orders = $state<OwnOrder[]>([]);
  let busyIds = $state<Set<string>>(new Set());
  let editingId = $state<string | null>(null);
  let editValue = $state(0);
  // Inline delete confirmation - the destructive click is one tap, the row
  // tints and the button becomes "Confirm"; a second tap (or the ×) resolves.
  let confirmId = $state<string | null>(null);
  // The same inline confirmation for the health queue's destructive fix. Kept
  // apart from `confirmId` on purpose: one order can appear in both the queue
  // and the table, and arming one must not arm the other.
  let healthConfirmId = $state<string | null>(null);
  // Which row's Delete button should take focus back once a cancel replaces the
  // confirmation. Null everywhere else, so an ordinary render cannot steal it.
  let restoreFocusTo = $state<string | null>(null);
  let bulkBusy = $state(false);

  // Each toast owns its auto-dismiss timer so
  // a manual dismiss can cancel it, and disposal clears everything pending -
  // this panel is conditionally rendered, so a stray timer would otherwise
  // fire after unmount.
  let toasts = $state<ToastMsg[]>([]);
  let toastSeq = 0;
  const toastTimers = new Map<number, number>();

  function pushToast(text: string, kind: 'error' | 'success' = 'success'): void {
    const id = ++toastSeq;
    toasts = [...toasts, { id, kind, text }];
    toastTimers.set(id, window.setTimeout(() => dismissToast(id), 4500));
  }

  function dismissToast(id: number): void {
    const timer = toastTimers.get(id);
    if (timer !== undefined) {
      window.clearTimeout(timer);
      toastTimers.delete(id);
    }
    toasts = toasts.filter((t) => t.id !== id);
  }

  function dispose() {
    liveTop.dispose();
    for (const timer of toastTimers.values()) window.clearTimeout(timer);
    toastTimers.clear();
  }

  // Stale-async guard, same shape as App.svelte's verifyGen: only the newest
  // load may commit. Two GET /orders can be in flight at once (e.g. a manual
  // Refresh during a slow load), and without this an older response landing
  // second overwrites the newer one.
  let loadGen = 0;

  function loadOrders(): void {
    const gen = ++loadGen;
    phase = 'loading';
    error = null;
    input.transport.fetchOrders()
      .then((r) => {
        if (gen !== loadGen) return;
        orders = r;
        phase = 'done';
      })
      .catch((e: unknown) => {
        if (gen !== loadGen) return;
        // A locked/no-login session is an auth hand-off, not a load error: the
        // parent opens the dialog (silent keyring unlock first), and the
        // input.sessionEpoch bump re-fires this fetch on success. Kept distinct from
        // 'error' so a cancelled dialog leaves an actionable "unlock required"
        // state instead of a stale failure.
        if (e instanceof DesktopCmdError && (e.code === 'needs_login' || e.code === 'needs_unlock')) {
          phase = 'locked';
          input.onauthrequired?.(e.code);
          return;
        }
        error = humanError(e);
        phase = 'error';
      });
  }

  // Load on mount and again when the parent bumps input.sessionEpoch - a fetch that
  // was gated on needs_login/needs_unlock retries the moment the session is
  // unlocked (the transport is a boot-time constant, so nothing else retriggers).
  $effect(() => {
    void input.sessionEpoch;
    loadOrders();
  });

  function markBusy(id: string, on: boolean): void {
    const next = new Set(busyIds);
    if (on) next.add(id); else next.delete(id);
    busyIds = next;
  }

  // The desktop command relays WFM rejections as a per-order
  // {status:"error", message} body. Treating "no throw" as success applied
  // the edit locally while WFM kept the old value - silent desync.
  function assertOrderOk(r: unknown): void {
    const res = r as { status?: string; message?: string } | null;
    // Only a literal 'ok' is WFM accepting the change. 'pending' and
    // 'uncertain_mutation' mean the outcome is unknown; letting those fall through
    // as success is the same silent desync in a different disguise.
    if (res?.status === 'ok') return;
    if (res?.status === 'error') throw new Error(res.message || 'WFM rejected the update');
    throw new Error(
      `WFM did not confirm the update${res?.status ? ` (${res.status})` : ''}. Recheck My Orders before trusting this row.`,
    );
  }

  async function toggleVisible(o: OwnOrder): Promise<void> {
    if (o.visible == null) return;
    markBusy(o.id, true);
    try {
      assertOrderOk(await input.transport.updateOrder(o.id, { visible: !o.visible }));
      o.visible = !o.visible;
      orders = [...orders];
    } catch (e) {
      pushToast(`Couldn't toggle: ${humanError(e)}`, 'error');
    } finally {
      markBusy(o.id, false);
    }
  }

  function startEdit(o: OwnOrder): void {
    editingId = o.id;
    editValue = o.platinum;
  }

  async function saveEdit(o: OwnOrder): Promise<void> {
    const newPrice = Number(editValue);
    if (!newPrice || newPrice < 1) return;
    if (newPrice > MAX_PLATINUM) {
      pushToast(`Price ${newPrice}p is above the ${MAX_PLATINUM}p cap.`, 'error');
      return;
    }
    markBusy(o.id, true);
    try {
      assertOrderOk(await input.transport.updateOrder(o.id, { platinum: newPrice }));
      o.platinum = newPrice;
      orders = [...orders];
      editingId = null;
    } catch (e) {
      pushToast(`Couldn't update: ${humanError(e)}`, 'error');
    } finally {
      markBusy(o.id, false);
    }
  }

  async function removeOne(o: OwnOrder): Promise<void> {
    if (confirmId !== o.id) {
      confirmId = o.id;
      return;
    }
    confirmId = null;
    markBusy(o.id, true);
    try {
      await input.transport.deleteOrder(o.id);
      orders = orders.filter((x) => x.id !== o.id);
      pushToast(`Deleted ${itemName(o)}.`);
    } catch (e) {
      pushToast(`Couldn't delete: ${humanError(e)}`, 'error');
    } finally {
      markBusy(o.id, false);
    }
  }

  async function bulkSetVisible(visible: boolean): Promise<void> {
    if (bulkBusy) return;
    const ids = orders.filter((o) => o.visible != null && o.visible !== visible).map((o) => o.id);
    if (ids.length === 0) return;
    bulkBusy = true;
    try {
      const resp = await input.transport.bulkVisibility(ids, visible);
      // The server can skip rows (already in that state, gone since fetch), so the
      // count comes from the per-order results rather than ids.length - and only a
      // confirmed row may have its local state changed, or a failed row renders as
      // done while the live listing is untouched.
      const confirmed = new Set(
        (resp?.results ?? [])
          .filter((r) => r.status === 'ok' && r.order_id)
          .map((r) => r.order_id as string),
      );
      for (const o of orders) if (confirmed.has(o.id)) o.visible = visible;
      orders = [...orders];
      const ok = confirmed.size;
      pushToast(
        visible
          ? `${ok} listing${ok === 1 ? '' : 's'} made visible.`
          : `${ok} listing${ok === 1 ? '' : 's'} made hidden.`,
      );
    } catch (e) {
      pushToast(`Couldn't update visibility: ${humanError(e)}`, 'error');
    } finally {
      bulkBusy = false;
    }
  }

  // The input.market snapshot keys on slug, so an order that never resolves to one
  // simply cannot be price-checked. Same defensive shape as itemName.
  function itemSlug(o: OwnOrder): string {
    return o.slug ?? '';
  }

  // Orders whose price has drifted from the input.market. Recomputed whenever the
  // orders list or the snapshot changes - repricing one row drops it out.
  let drifted = $derived.by((): DriftRow[] => {
    const market = input.market;
    if (!market?.items) return [];
    return selectDrifted(
      orders
        .filter((o) => o.side !== 'buy')
        .map((o) => {
          const slug = itemSlug(o);
          return {
            id: o.id,
            slug,
            name: itemName(o),
            platinum: orderUnitPrice(o.platinum, o.per_trade) ?? NaN,
            type: o.side,
            m: slug && market.items[slug] ? cachedUnitMarket(market.items[slug], (o.per_trade ?? 1) > 1) : null,
          };
        })
        .filter((r) => r.slug !== '' && Number.isFinite(r.platinum)),
    );
  });

  // Applies the suggestion as a normal price edit - same transport call, same
  // success assertion, so a WFM rejection cannot silently desync the row.
  async function reprice(row: DriftRow): Promise<void> {
    const o = orders.find((x) => x.id === row.id);
    if (!o) return;
    markBusy(row.id, true);
    try {
      const total = orderLotPrice(row.suggested, o.per_trade);
      if (total == null || total > MAX_PLATINUM) throw new Error('Suggested lot total exceeds the listing price limit. Review the lot on WFM.');
      assertOrderOk(await input.transport.updateOrder(row.id, { platinum: total }));
      o.platinum = total;
      orders = [...orders];
      pushToast(`${row.name} repriced to ${total}p per lot (${row.suggested}p / unit).`);
    } catch (e) {
      pushToast(`Couldn't reprice: ${humanError(e)}`, 'error');
    } finally {
      markBusy(row.id, false);
    }
  }

  // ---- Listing health (live top-of-book + owned quantities) ----
  // "Check live" asks WFM for the exact-tier top-of-book of every SELL listing
  // with the user's own order already excluded (wfm-core does that by
  // username), then `assessListings` turns it plus the last scan's owned
  // counts into concrete fixes.
  let fixAllBusy = $state(false);

  function liveForOrder(o: OwnOrder): LiveTop | null {
    return liveTop.get({ slug: itemSlug(o), rank: o.rank, subtype: o.subtype }) ?? null;
  }

  async function checkLive(): Promise<void> {
    const targets = orders.filter((o) => o.side !== 'buy' && itemSlug(o) !== '');
    await liveTop.check(targets.map((o) => ({ slug: itemSlug(o), rank: o.rank ?? 0, subtype: o.subtype ?? null })));
  }

  // Composed items - prime sets are assembled from parts, so a scan of
  // individual items can never report the set itself. See `ownedEvidence`.
  let composedSlugs = $derived(input.market?.set_to_parts ? new Set(Object.keys(input.market.set_to_parts)) : null);

  let health = $derived.by((): HealthIssue[] => {
    if (liveTop.quotes.size === 0 && !input.ownedQty) return [];
    return assessListings(
      orders
        .filter((o) => o.side !== 'buy')
        .map((o) => {
          const slug = itemSlug(o);
          return {
            id: o.id, slug, name: itemName(o),
            platinum: orderUnitPrice(o.platinum, o.per_trade) ?? NaN, quantity: o.quantity, type: 'sell' as const,
            live: slug ? liveForOrder(o) : null,
            owned: slug ? ownedEvidence(slug, o.subtype ?? null, input.ownedQty, composedSlugs) : null,
          };
        })
        .filter((r) => r.slug !== '' && Number.isFinite(r.platinum)),
    );
  });
  let healthSummary = $derived(summarize(health));
  $effect(() => {
    input.onsummary?.(phase === 'done' ? { live: orders.length, issues: health.length } : null);
  });

  async function applyFix(issue: HealthIssue): Promise<void> {
    const o = orders.find((x) => x.id === issue.id);
    if (!o) return;
    markBusy(issue.id, true);
    try {
      if (issue.kind === 'overpriced' || issue.kind === 'underbid') {
        const p = orderLotPrice(Math.max(MIN_PLATINUM, issue.suggested), o.per_trade);
        if (p == null || p > MAX_PLATINUM) throw new Error('Suggested lot total exceeds the listing price limit. Review the lot on WFM.');
        assertOrderOk(await input.transport.updateOrder(issue.id, { platinum: p }));
        o.platinum = p;
        pushToast(`${issue.name} repriced to ${p}p per lot.`);
      } else if (issue.kind === 'excess-qty') {
        assertOrderOk(await input.transport.updateOrder(issue.id, { quantity: issue.suggested }));
        o.quantity = issue.suggested;
        pushToast(`${issue.name} quantity set to ${issue.suggested}.`);
      } else if (issue.kind === 'not-owned') {
        healthConfirmId = null;
        await input.transport.deleteOrder(issue.id);
        orders = orders.filter((x) => x.id !== issue.id);
        pushToast(`Deleted ${issue.name}.`);
        return;
      }
      orders = [...orders];
    } catch (e) {
      pushToast(`Couldn't fix ${issue.name}: ${humanError(e)}`, 'error');
    } finally {
      markBusy(issue.id, false);
    }
  }

  /** Apply every PRICE fix (match lowest ask / meet the bid). Quantity and
   *  delete fixes stay one-click-each - those change what's for sale. */
  async function fixAllPrices(): Promise<void> {
    if (fixAllBusy) return;
    fixAllBusy = true;
    try {
      for (const issue of health.filter((i) => i.kind === 'overpriced' || i.kind === 'underbid')) {
        await applyFix(issue);
      }
    } finally {
      fixAllBusy = false;
    }
  }

  // A missing catalogue entry still leaves the validated item id visible.
  function itemName(o: OwnOrder): string {
    return (
      o.name ||
      o.slug ||
      o.item_id ||
      'unknown'
    );
  }

  // ---- Show / Narrow controls on the orders table's bar ----
  type Show = 'all' | 'sell' | 'buy' | 'hidden' | 'issues';
  let show = $state<Show>('all');
  let nameFilter = $state('');
  // Orders with something in the fix queue (a live/scan health issue, or the
  // snapshot-drift fallback), so the "Issues" segment narrows to them.
  let issueIds = $derived.by(() => {
    const ids = new Set<string>();
    for (const h of health) ids.add(h.id);
    if (liveTop.quotes.size === 0) for (const d of drifted) ids.add(d.id);
    return ids;
  });
  let counts = $derived({
    all: orders.length,
    sell: orders.filter((o) => o.side !== 'buy').length,
    buy: orders.filter((o) => o.side === 'buy').length,
    hidden: orders.filter((o) => o.visible === false).length,
    issues: issueIds.size,
  });
  let shown = $derived.by(() => {
    const f = nameFilter.trim().toLowerCase();
    return orders.filter((o) => {
      if (show === 'sell' && o.side === 'buy') return false;
      if (show === 'buy' && o.side !== 'buy') return false;
      if (show === 'hidden' && o.visible !== false) return false;
      if (show === 'issues' && !issueIds.has(o.id)) return false;
      return !f || itemName(o).toLowerCase().includes(f);
    });
  });
  let listedValue = $derived(orders.filter((o) => o.side !== 'buy').reduce((a, o) => a + (orderUnitPrice(o.platinum, o.per_trade) ?? 0) * o.quantity, 0));

  // The fix queue: live/scan health issues first, then (only while no live
  // check has run) the snapshot-drift fallback for the remaining sell orders.
  // Once a live check has run, Listing health covers the same orders with
  // exact figures, so the two never show together.
  type QueueRow =
    | { key: string; id: string; name: string; slug: string; kind: 'health'; h: HealthIssue }
    | { key: string; id: string; name: string; slug: string; kind: 'drift'; d: DriftRow };
  let queue = $derived.by((): QueueRow[] => {
    const rows: QueueRow[] = health.map((h) => ({ key: `h:${h.id}:${h.kind}`, id: h.id, name: h.name, slug: h.slug, kind: 'health', h }));
    if (liveTop.quotes.size === 0) {
      for (const d of drifted) rows.push({ key: `d:${d.id}`, id: d.id, name: d.name, slug: d.slug, kind: 'drift', d });
    }
    return rows;
  });
  function orderById(id: string): OwnOrder | undefined {
    return orders.find((o) => o.id === id);
  }
  function healthAction(h: HealthIssue): string {
    return h.kind === 'not-owned' ? 'Delete' : h.kind === 'excess-qty' ? 'Set qty' : 'Reprice';
  }
  // The queue's destructive fix arms first, matching the table's inline delete.
  // A repricing or quantity fix is reversible, so it stays single-click.
  function armOrFix(h: HealthIssue): void {
    if (h.kind === 'not-owned') {
      restoreFocusTo = null;
      healthConfirmId = h.id;
      return;
    }
    void applyFix(h);
  }
  function cancelDelete(id: string): void {
    healthConfirmId = null;
    restoreFocusTo = id;
  }
  // Arming replaces the control the user activated, and cancelling replaces it
  // back. Focus follows the replacement so a keyboard user keeps their place
  // instead of being dropped on the document body.

  function driftWhy(d: DriftRow): string {
    const pct = Math.round(d.delta_pct * 100);
    return d.kind === 'overpriced'
      ? `${pct}% above the last snapshot's clearing price - a starting point, not a quote${d.thin ? ' (thin book)' : ''}.`
      : `${pct}% under the last snapshot's clearing price - you may be leaving plat on the table${d.thin ? ' (thin book)' : ''}.`;
  }
  return {
    dismissToast,
    dispose,
    loadOrders,
    toggleVisible,
    startEdit,
    saveEdit,
    removeOne,
    bulkSetVisible,
    reprice,
    liveForOrder,
    checkLive,
    applyFix,
    fixAllPrices,
    itemName,
    orderById,
    healthAction,
    armOrFix,
    cancelDelete,
    driftWhy,
    get phase() { return phase; },
    get error() { return error; },
    get orders() { return orders; },
    get busyIds() { return busyIds; },
    get editingId() { return editingId; },
    set editingId(value: typeof editingId) { editingId = value; },
    get editValue() { return editValue; },
    set editValue(value: typeof editValue) { editValue = value; },
    get confirmId() { return confirmId; },
    set confirmId(value: typeof confirmId) { confirmId = value; },
    get healthConfirmId() { return healthConfirmId; },
    set healthConfirmId(value: typeof healthConfirmId) { healthConfirmId = value; },
    get restoreFocusTo() { return restoreFocusTo; },
    set restoreFocusTo(value: typeof restoreFocusTo) { restoreFocusTo = value; },
    get bulkBusy() { return bulkBusy; },
    get toasts() { return toasts; },
    get drifted() { return drifted; },
    get fixAllBusy() { return fixAllBusy; },
    get health() { return health; },
    get healthSummary() { return healthSummary; },
    get show() { return show; },
    set show(value: typeof show) { show = value; },
    get nameFilter() { return nameFilter; },
    set nameFilter(value: typeof nameFilter) { nameFilter = value; },
    get counts() { return counts; },
    get shown() { return shown; },
    get listedValue() { return listedValue; },
    get queue() { return queue; },
    get liveTop() { return liveTop; },
  };
}
