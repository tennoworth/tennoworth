<script lang="ts">
import type { Snippet } from 'svelte';
import type { InventoryController } from '../features/inventory/controller.svelte';
import type { ListingController } from '../features/selling/controller.svelte';
import type { FilterController } from '../features/selling/filters.svelte';
import { baroPhase } from '../domain/baro-board';
import { humanWindow } from '../ui/format';
  let { inShell, inventory, listing, filters, unresolvedCount, unresolvedSummary, inventoryFreshness, inventoryStaleness, inventoryTimestamp, marketFreshness, marketStaleness, ordersToFix, baroState, unreadNotifications, wfmLabel, projectLinkAnchors, onexport, onimport, onclear, onupdates, onfeedback, onauth }: {
    inShell: boolean;
    inventory: InventoryController;
    listing: ListingController;
    filters: FilterController;
    unresolvedCount: number;
    unresolvedSummary: string;
    inventoryFreshness: string;
    inventoryStaleness: string | null;
    inventoryTimestamp: string | null;
    marketFreshness: string;
    marketStaleness: string | null;
    ordersToFix: number;
    baroState: ReturnType<typeof baroPhase> | null;
    unreadNotifications: number;
    wfmLabel: string;
    projectLinkAnchors: Snippet;
    onexport: () => void;
    onimport: () => void;
    onclear: () => void;
    onupdates: () => void;
    onfeedback: () => void;
    onauth: (code: 'needs_unlock' | 'needs_login') => void;
  } = $props();
  function headerClearance(node: HTMLElement, inShell: boolean) {
    const root = document.documentElement;
    let enabled = inShell;
    const update = () => {
      if (enabled && getComputedStyle(node).position === 'sticky') root.style.setProperty('--sticky-header-clearance', `${node.offsetHeight}px`);
      else root.style.removeProperty('--sticky-header-clearance');
    };
    const observer = new ResizeObserver(update);
    observer.observe(node);
    update();
    return {
      update(value: boolean) { enabled = value; update(); },
      destroy() { observer.disconnect(); root.style.removeProperty('--sticky-header-clearance'); },
    };
  }

  let moreOpen = $state(false);
  let moreTrigger = $state<HTMLButtonElement>();
  export function focusMore() { moreTrigger?.focus(); }
  $effect(() => {
    if (!moreOpen) return;
    const click = (e: MouseEvent): void => {
      if (!(e.target as HTMLElement | null)?.closest('.more-pop, .more-trigger')) moreOpen = false;
    };
    const key = (e: KeyboardEvent): void => {
      if (e.key !== 'Escape') return;
      moreOpen = false;
      moreTrigger?.focus();
    };
    document.addEventListener('click', click, true);
    document.addEventListener('keydown', key);
    return () => { document.removeEventListener('click', click, true); document.removeEventListener('keydown', key); };
  });
  let refreshOpen = $state(false);
  async function refreshFromGame() {
    refreshOpen = false;
    await inventory.pullInventory();
  }
  $effect(() => {
    if (!refreshOpen) return;
    const handler = (e: MouseEvent) => {
      const t = e.target instanceof Element ? e.target : null;
      if (!t?.closest('.refresh-pop, .refresh-trigger')) refreshOpen = false;
    };
    document.addEventListener('click', handler, true);
    return () => document.removeEventListener('click', handler, true);
  });


</script>

  <!-- Shell-level status strip: one 40px spine on the landing AND the
       workspace. In the shell its brand cell sits exactly over the sidebar
       column; the rest answers "is what I'm looking at still true?" -
       inventory age, market age, orders to fix, Baro, WFM session. Rare
       inventory actions (Export / Restore / Clear) live one click deeper in
       the Refresh menu. -->
  <header data-shell class="statusbar" class:shell-strip={inShell} use:headerClearance={inShell}>
    <div data-shell class="brand">
      <h1 data-shell>TennoWorth</h1>
      {#if !inShell}<span data-shell class="sub">warframe.market prices, ranked by what actually sells</span>{/if}
    </div>
    
      <div data-shell class="cell inv" title={unresolvedCount > 0 ? `${unresolvedCount} items couldn't be price-matched (${unresolvedSummary}) - usually untradeable blueprints, quest items and very new content.` : undefined}>
        {#if inventory.inventoryName}
          <span data-shell class="dot {inventory.refreshFailed ? 'stale' : inventoryFreshness}" role="img" aria-label={inventory.refreshFailed ? 'Inventory refresh failed' : `Inventory recorded ${inventoryStaleness ?? 'at an unknown time'}`}></span>
          <span data-shell>{inventory.source === 'import' ? 'Imported inventory' : inventory.source === 'saved' || inventory.refreshFailed || inventory.noTradeables ? 'Using saved scan' : 'Inventory'}</span>
          <b data-shell class="file" title={inventory.inventoryName}>{inventory.inventoryName}</b>
          {#if inventoryTimestamp}
            <time data-shell datetime={inventoryTimestamp}>As of {new Date(inventoryTimestamp).toLocaleString(undefined, { month: 'short', day: 'numeric', year: 'numeric', hour: '2-digit', minute: '2-digit' })} · {inventoryStaleness}</time>
          {:else}<span data-shell>Timestamp unavailable</span>{/if}
          {#if inventory.refreshFailed}<span data-shell class="bad">Last refresh failed</span>{:else if inventory.noTradeables}<span data-shell>No tradeable items found; showing saved inventory</span>{/if}
        {:else}
          <span data-shell class="dot" aria-hidden="true"></span>
          <span data-shell>No inventory yet</span>
        {/if}
        <div data-shell class="refresh-wrap">
          <!-- Reflects the scan itself, not just the menu: refreshFromGame
               closes the popover before awaiting, so the "Scanning game…"
               label inside it vanished the moment it mattered and a ~10s scan
               looked like a dead click. This trigger stays on screen. -->
          <button data-shell
            class="refresh-trigger"
            class:busy={inventory.pullingInventory}
            onclick={() => (refreshOpen = !refreshOpen)}
            aria-expanded={refreshOpen}
            aria-busy={inventory.pullingInventory}
            disabled={inventory.pullingInventory}
            title={inventory.pullingInventory
              ? 'Reading the running game’s memory - this can take a few seconds.'
              : 'Load fresh inventory - re-fetch from the game. Export / Restore / Clear live in this menu too.'}
          >{inventory.pullingInventory ? 'Scanning…' : 'Refresh ▾'}</button>
          {#if refreshOpen}
            <div data-shell class="refresh-pop">
              <p data-shell class="rp-lede">Scan the running game - no file needed.</p>
              <button data-shell class="rp-primary" data-testid="desktop-scan" onclick={refreshFromGame} disabled={inventory.pullingInventory}>
                {inventory.pullingInventory ? 'Scanning game…' : 'Scan game'}
              </button>
              <div data-shell class="rp-sep" aria-hidden="true"></div>
              {#if inventory.inventoryName}
                <button data-shell class="rp-item" onclick={() => { refreshOpen = false; onexport(); }} title="Download an encrypted snapshot for another device or backup.">Export…</button>
              {/if}
              <button data-shell class="rp-item" onclick={() => { refreshOpen = false; onimport(); }} title="Restore an encrypted snapshot exported from another device.">Restore…</button>
              {#if inventory.inventoryName}
                <button data-shell class="rp-item danger" onclick={() => { refreshOpen = false; onclear(); }} title="Forget the saved inventory entirely.">Clear</button>
              {/if}
              {#if unresolvedCount > 0}
                <p data-shell class="rp-note" title="Breakdown: {unresolvedSummary}.">{unresolvedCount} items couldn't be price-matched (not shown) - usually untradeable blueprints, quest items and very new content; your sellable items aren't affected.</p>
              {/if}
            </div>
          {/if}
        </div>
      </div>
    
    <div data-shell class="cell">
      {#if marketFreshness === 'stale'}
        <span data-shell>Market</span>
        <span data-shell class="tag stale">Stale · {marketStaleness}</span>
      {:else}
        <span data-shell class="dot {marketFreshness}" role="img" aria-label="Market data {marketFreshness}"></span>
        <span data-shell>Market</span>
        <b data-shell>{marketStaleness ?? '-'}</b>
        {#if marketFreshness !== 'unknown'}<span data-shell>· {marketFreshness}</span>{/if}
      {/if}
    </div>
    {#if inShell && ordersToFix > 0}
      <div data-shell class="cell attn">
        <b data-shell>{ordersToFix}</b>
        <span data-shell>{ordersToFix === 1 ? 'order' : 'orders'} to fix</span>
        <button data-shell type="button" class="link" onclick={() => filters.setView('orders')} aria-label="Open My orders">→</button>
      </div>
    {/if}
    {#if baroState && baroState.phase !== 'unknown'}
      <div data-shell class="cell baro">
        <span data-shell class="ducat" aria-hidden="true">⌬</span>
        <span data-shell>{baroState.phase === 'here' ? 'Baro leaves in' : 'Baro arrives in'}</span>
        <b data-shell>{humanWindow(baroState.windowMs)}</b>
      </div>
    {/if}
    <span data-shell class="grow"></span>
    {#if !inShell}
      <div data-shell class="cell">
        <button data-shell type="button" class="cellbtn" onclick={() => filters.setView('notifications')}>Notifications{#if unreadNotifications}<span data-shell class="badge unread">{unreadNotifications}</span>{/if}</button>
      </div>
      <div data-shell class="cell">
        <button data-shell type="button" class="cellbtn" onclick={() => filters.setView('settings')}><span data-shell aria-hidden="true">⚙</span>Settings</button>
      </div>
      <div data-shell class="cell end more">
        <button data-shell type="button" class="cellbtn more-trigger" bind:this={moreTrigger} aria-expanded={moreOpen} aria-controls="more-pop" onclick={() => (moreOpen = !moreOpen)}>More ▾</button>
        {#if moreOpen}
          <div data-shell id="more-pop" class="more-pop">
            <button data-shell type="button" onclick={() => { moreOpen = false; onupdates(); }}>Check for updates</button>
            <button data-shell type="button" onclick={() => { moreOpen = false; onfeedback(); }}>Send feedback</button>
            <hr data-shell />
            <a data-shell href="#faq" onclick={() => (moreOpen = false)}>FAQ</a>
            {@render projectLinkAnchors()}
          </div>
        {/if}
      </div>
    {:else}
      <div data-shell class="cell end">
        <span data-shell>WFM</span>
        {#if listing.wfmStatus && !listing.wfmStatus.unlocked}
          <button data-shell type="button" class="link" onclick={() => onauth(listing.wfmStatus?.logged_in ? 'needs_unlock' : 'needs_login')}>{wfmLabel}</button>
        {:else}
          <b data-shell>{wfmLabel}</b>
        {/if}
        {#if listing.ordersSummary}<span data-shell>· {listing.ordersSummary.live} live</span>{/if}
      </div>
    {/if}
  </header>
