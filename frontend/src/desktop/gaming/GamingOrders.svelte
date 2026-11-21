<script lang="ts">
  import { untrack } from 'svelte';
  import { t } from '../../i18n/i18n.svelte';
  import { createOrdersController, type OrdersInput } from '../orders/controller.svelte';
  import { useDesktopServices } from '../shell/desktop-context';

  let { input, active, wfmStatus, onconnect }: {
    input: Omit<OrdersInput, 'active'>;
    active: boolean;
    wfmStatus: { logged_in: boolean; unlocked: boolean } | null;
    onconnect: () => void;
  } = $props();
  // Getters, so the controller follows the live props rather than a copy.
  const controller = untrack(() => createOrdersController({
    get transport() { return input.transport; },
    get market() { return input.market; },
    get ownedQty() { return input.ownedQty; },
    get sessionEpoch() { return input.sessionEpoch; },
    get onauthrequired() { return input.onauthrequired; },
    get onsummary() { return input.onsummary; },
    get healthPrefs() { return input.healthPrefs; },
    get active() { return active; },
  }, useDesktopServices()));

  // Fixes need live prices; read them once per load rather than on request,
  // because a pad has no spare button for it.
  let checkedFor: unknown = null;
  $effect(() => {
    const orders = controller.orders;
    if (controller.phase !== 'done' || !orders.length || !active) return;
    untrack(() => {
      if (checkedFor === orders) return;
      checkedFor = orders;
      void controller.checkLive();
    });
  });

  let issueIds = $derived(new Set(controller.health.map((issue) => issue.id)));
  let others = $derived(controller.orders.filter((o) => o.side !== 'buy' && !issueIds.has(o.id)));
  let connected = $derived(!!wfmStatus?.unlocked);
  export function secondary() { if (controller.health.length && !controller.fixAllBusy) void controller.fixAllPrices(); }
</script>

<section class="gm-section" aria-labelledby="gm-orders-title">
  <header class="gm-head">
    <h2 id="gm-orders-title">{t('gaming.orders')}</h2>
    {#if controller.phase === 'done'}
      <p class="muted" role="status">{t('gaming.ordersCount', { count: controller.orders.length })} · {controller.health.length ? t('gaming.ordersIssues', { count: controller.health.length }) : t('gaming.ordersFine')}</p>
      {#if controller.health.length}<button type="button" class="gm-action" disabled={controller.fixAllBusy} onclick={secondary}><kbd class="gm-key">X</kbd>{t('gaming.fixAll')}</button>{/if}
    {/if}
  </header>
  {#if !connected && controller.phase !== 'done'}
    <div class="gm-empty"><p>{t('gaming.ordersLocked')}</p><button type="button" class="gm-action primary" data-pad-default onclick={onconnect}>{t('gaming.connect')}</button></div>
  {:else if (controller.phase === 'loading' || controller.phase === 'idle') && !controller.orders.length}
    <div class="gm-empty"><p>{t('gaming.loadingOrders')}</p></div>
  {:else if controller.phase === 'error'}
    <div class="gm-empty"><p role="alert">{controller.error}</p></div>
  {:else if controller.phase === 'done' && !controller.orders.length}
    <div class="gm-empty"><p>{t('gaming.noOrders')}</p></div>
  {:else}
    <ul class="gm-list" data-pad-scroll>
      {#each controller.health as issue, index (issue.id)}
        <li>
          <button type="button" class="gm-row issue" data-pad-default={index === 0 ? '' : undefined} disabled={controller.busyIds.has(issue.id)} onclick={() => { if (controller.healthConfirmId === issue.id) { controller.healthConfirmId = null; void controller.applyFix(issue); } else controller.armOrFix(issue); }}>
            <span class="gm-main">
              <span class="gm-name">{issue.name}</span>
              <span class="gm-sub">{controller.healthWhy(issue)}</span>
            </span>
            <span class="gm-verb">{controller.healthConfirmId === issue.id ? t('orders.confirm') : controller.healthAction(issue)}</span>
          </button>
        </li>
      {/each}
      {#each others as order (order.id)}
        <li>
          <button type="button" class="gm-row" disabled={controller.busyIds.has(order.id)} onclick={() => void controller.toggleVisible(order)}>
            <span class="gm-main">
              <span class="gm-name">{controller.itemName(order)}</span>
              <span class="gm-sub mono">{t('gaming.priceQty', { price: order.platinum, quantity: order.quantity })}{#if order.visible === false}{' · '}{t('gaming.hidden')}{/if}</span>
            </span>
            <span class="gm-verb">{order.visible === false ? t('gaming.show') : t('gaming.hide')}</span>
          </button>
        </li>
      {/each}
    </ul>
  {/if}
</section>
