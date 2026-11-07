<script lang="ts">
  import { t } from '../../i18n/i18n.svelte';
  const STATE_WORDS = { fresh: 'strip.state.fresh', aging: 'strip.state.aging', stale: 'strip.state.stale', unknown: 'strip.state.unknown' } as const;
  const stateWord = (state: string) => t(STATE_WORDS[state as keyof typeof STATE_WORDS] ?? 'strip.state.unknown');
import type { Snippet } from 'svelte';
import type { InventoryController } from '../inventory/controller.svelte';
import type { ListingController } from '../selling/controller.svelte';
import type { FilterController } from '../selling/filters.svelte';
import { baroPhase } from '../../domain/baro-board';
import { humanWindow, shortStamp } from '../../ui/format';
import PresenceMenu from '../presence/PresenceMenu.svelte';
import { busy, closeOnFocusOut, focusLater } from '../../ui/keyboard-actions';
import type { ActivityLog } from './activity.svelte';
import type { PresenceController } from '../presence/presence.svelte';
  let { inShell, inventory, listing, filters, unresolvedCount, unresolvedSummary, inventoryFreshness, inventoryStaleness, inventoryTimestamp, marketFreshness, marketStaleness, baroState, unreadNotifications, wfmLabel, projectLinkAnchors, onexport, onimport, onclear, onupdates, onfeedback, onauth, presence, onpresencesettings, activity, keyboardTools }: {
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
    baroState: ReturnType<typeof baroPhase> | null;
    unreadNotifications: number;
    wfmLabel: string;
    projectLinkAnchors: Snippet;
    keyboardTools?: Snippet;
    onexport: () => void;
    onimport: () => void;
    onclear: () => void;
    onupdates: () => void;
    onfeedback: () => void;
    onauth: (code: 'needs_unlock' | 'needs_login') => void;
    /** Desktop only: the user's warframe.market status, shown once unlocked. */
    presence?: PresenceController;
    onpresencesettings?: () => void;
    /** Desktop only: what the app is doing, or last finished. */
    activity?: ActivityLog;
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

  // The workspace strip stays one line at every desktop width and in every
  // state. When it would wrap, it hides its least useful words a step at a
  // time (fit-1 first); they stay readable to assistive technology, and the
  // Refresh menu repeats the scan name. The last step stops wrapping and lets
  // the activity message shorten, unless even that would clip. Narrow
  // windows keep the stacked strip.
  const FIT_STEPS = 4;
  function fitOneLine(node: HTMLElement, enabled: boolean) {
    const wide = matchMedia('(min-width: 901px)');
    let frame = 0;
    const fit = () => {
      frame = 0;
      if (!enabled || !wide.matches) { delete node.dataset.fit; return; }
      const line = parseFloat(getComputedStyle(node).minHeight) + 1.5;
      let step = 0;
      node.dataset.fit = '0';
      while (step < FIT_STEPS && node.getBoundingClientRect().height > line) node.dataset.fit = String(++step);
      // Enlarged text can leave even the last step too wide; wrapping then
      // beats clipping status off the edge.
      if (step === FIT_STEPS && node.scrollWidth > node.clientWidth + 1) node.dataset.fit = String(FIT_STEPS - 1);
    };
    const schedule = () => { if (!frame) frame = requestAnimationFrame(fit); };
    const resize = new ResizeObserver(schedule);
    resize.observe(node);
    const content = new MutationObserver(schedule);
    content.observe(node, { childList: true, subtree: true, characterData: true });
    wide.addEventListener('change', schedule);
    void document.fonts?.ready.then(schedule);
    fit();
    return {
      update(value: boolean) { enabled = value; fit(); },
      destroy() {
        resize.disconnect(); content.disconnect(); wide.removeEventListener('change', schedule);
        if (frame) cancelAnimationFrame(frame);
        delete node.dataset.fit;
      },
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
  let refreshTrigger = $state<HTMLButtonElement>();
  // Each action closes the menu and so removes the button that had focus;
  // focus goes back to Refresh first, so the next Tab continues from there.
  function closeRefresh() { refreshTrigger?.focus(); refreshOpen = false; confirmingClear = false; }
  async function refreshFromGame() {
    closeRefresh();
    await inventory.pullInventory();
  }
  // Clear sits one Tab after Restore and erases the saved scan, so it asks.
  let confirmingClear = $state(false);
  let keepScan = $state<HTMLButtonElement>();
  function clearScan() {
    closeRefresh();
    onclear();
    focusLater(() => document.querySelector<HTMLElement>('[data-testid="desktop-scan"]') ?? refreshTrigger);
  }
  $effect(() => {
    if (!refreshOpen) return;
    const handler = (e: MouseEvent) => {
      const t = e.target instanceof Element ? e.target : null;
      if (!t?.closest('.refresh-pop, .refresh-trigger')) refreshOpen = false;
    };
    // Same as the More menu beside it: Escape closes and returns focus.
    const key = (e: KeyboardEvent): void => {
      if (e.key !== 'Escape') return;
      refreshOpen = false;
      refreshTrigger?.focus();
    };
    document.addEventListener('click', handler, true);
    document.addEventListener('keydown', key);
    return () => { document.removeEventListener('click', handler, true); document.removeEventListener('keydown', key); };
  });


</script>

  <!-- Shell-level status strip: one 40px spine on the landing AND the
       workspace. In the shell its brand cell sits exactly over the sidebar
       column; the rest answers "is what I'm looking at still true?" -
       inventory age, market age, orders to fix, Baro, WFM session. Rare
       inventory actions (Export / Restore / Clear) live one click deeper in
       the Refresh menu. -->
  <header data-shell class="statusbar" class:shell-strip={inShell} use:headerClearance={inShell} use:fitOneLine={inShell}>
    <div data-shell class="brand">
      <h1 data-shell>TennoWorth</h1>
      {#if !inShell}<span data-shell class="sub">{t('strip.tagline')}</span>{/if}
    </div>
    
      <div data-shell class="cell inv" title={unresolvedCount > 0 ? t('strip.unmatched', { count: unresolvedCount, summary: unresolvedSummary }) : undefined}>
        {#if inventory.inventoryName}
          <span data-shell class="dot {inventory.refreshFailed ? 'stale' : inventoryFreshness}" role="img" aria-label={inventory.refreshFailed ? t('strip.inventoryFailedLabel') : inventoryStaleness ? t('strip.inventoryRecordedLabel', { age: inventoryStaleness }) : t('strip.inventoryUnknownTime')}></span>
          <span data-shell class="fit-2">{inventory.source === 'import' ? t('strip.imported') : inventory.source === 'saved' || inventory.refreshFailed || inventory.noTradeables ? t('strip.savedScan') : t('strip.inventory')}</span>
          <b data-shell class="file fit-1" title={inventory.inventoryName}>{inventory.inventoryName}</b>
          {#if inventoryTimestamp}
            <time data-shell datetime={inventoryTimestamp} title={new Date(inventoryTimestamp).toLocaleString()}><span data-shell class="fit-2">{shortStamp(inventoryTimestamp)} · </span>{inventoryStaleness}</time>
          {:else}<span data-shell>{t('strip.timestampUnavailable')}</span>{/if}
          {#if inventory.refreshFailed}<span data-shell class="note bad" title={t('strip.refreshFailed')}>{t('strip.refreshFailed')}</span>{:else if inventory.noTradeables}<span data-shell class="note" title={t('strip.noTradeables')}>{t('strip.noTradeables')}</span>{/if}
        {:else}
          <span data-shell class="dot" aria-hidden="true"></span>
          <span data-shell>{inventory.restored ? t('strip.noInventory') : t('strip.loadingScan')}</span>
        {/if}
        <div data-shell class="refresh-wrap">
          <!-- Reflects the scan itself, not just the menu: refreshFromGame
               closes the popover before awaiting, so the "Scanning game…"
               label inside it vanished the moment it mattered and a ~10s scan
               looked like a dead click. This trigger stays on screen. -->
          <button data-shell
            class="refresh-trigger"
            bind:this={refreshTrigger}
            class:busy={inventory.pullingInventory}
            onclick={() => (refreshOpen = !refreshOpen)}
            aria-expanded={refreshOpen}
            aria-controls="refresh-pop"
            aria-busy={inventory.pullingInventory}
            use:busy={inventory.pullingInventory}
            title={inventory.pullingInventory
              ? t('strip.refreshingTitle')
              : t('strip.refreshTitle')}
          >{inventory.pullingInventory ? t('strip.refreshing') : t('strip.refresh')}</button>
          {#if refreshOpen}
            <div data-shell id="refresh-pop" class="refresh-pop" use:closeOnFocusOut={{ trigger: () => refreshTrigger, close: () => { refreshOpen = false; confirmingClear = false; } }}>
              {#if inventory.inventoryName}<p data-shell class="rp-name">{inventory.inventoryName}</p>{/if}
              <p data-shell class="rp-lede">{t('strip.menuLede')}</p>
              <button data-shell class="rp-primary" data-testid="desktop-scan" onclick={refreshFromGame} use:busy={inventory.pullingInventory}>
                {inventory.pullingInventory ? t('firstRun.scanning') : t('strip.scanGame')}
              </button>
              <div data-shell class="rp-sep" aria-hidden="true"></div>
              {#if inventory.inventoryName}
                <button data-shell class="rp-item" onclick={() => { closeRefresh(); onexport(); }} title={t('strip.exportTitle')}>{t('strip.export')}</button>
              {/if}
              <button data-shell class="rp-item" onclick={() => { closeRefresh(); onimport(); }} title={t('strip.restoreTitle')}>{t('strip.restore')}</button>
              {#if inventory.inventoryName}
                {#if confirmingClear}
                  <div data-shell class="rp-confirm" role="group" aria-labelledby="rp-clear-q">
                    <p data-shell id="rp-clear-q" class="rp-note">{t('strip.clearAsk')}</p>
                    <button data-shell class="rp-item danger" onclick={clearScan}>{t('strip.clearConfirm')}</button>
                    <button data-shell class="rp-item" bind:this={keepScan} onclick={() => { confirmingClear = false; focusLater(() => document.querySelector<HTMLElement>('#refresh-pop .rp-item.danger')); }}>{t('strip.clearKeep')}</button>
                  </div>
                {:else}
                  <button data-shell class="rp-item danger" onclick={() => { confirmingClear = true; focusLater(() => keepScan); }} title={t('strip.clearTitle')}>{t('strip.clear')}</button>
                {/if}
              {/if}
              {#if unresolvedCount > 0}
                <p data-shell class="rp-note" title={t('strip.unmatchedBreakdown', { summary: unresolvedSummary })}>{t('strip.unmatchedNote', { count: unresolvedCount })}</p>
              {/if}
            </div>
          {/if}
        </div>
      </div>
    
    <div data-shell class="cell">
      {#if marketFreshness === 'stale'}
        <span data-shell class="fit-3">{t('strip.market')}</span>
        <span data-shell class="tag stale">{t('strip.stale', { age: marketStaleness ?? '-' })}</span>
      {:else}
        <span data-shell class="dot {marketFreshness}" role="img" aria-label={t('strip.marketLabel', { state: stateWord(marketFreshness) })}></span>
        <span data-shell class="fit-3">{t('strip.market')}</span>
        <b data-shell>{marketStaleness ?? '-'}</b>
        {#if marketFreshness !== 'unknown'}<span data-shell class="fit-1">· {stateWord(marketFreshness)}</span>{/if}
      {/if}
    </div>
    <!-- In the workspace, orders to fix and Baro's countdown are sidebar
         badges beside the views they open; the strip keeps them only on the
         landing, which has no sidebar. -->
    {#if !inShell && baroState && baroState.phase !== 'unknown'}
      <div data-shell class="cell baro">
        <span data-shell class="ducat" aria-hidden="true">⌬</span>
        <span data-shell>{baroState.phase === 'here' ? t('strip.baroLeaves') : t('strip.baroArrives')}</span>
        <b data-shell>{humanWindow(baroState.windowMs)}</b>
      </div>
    {/if}
    <span data-shell class="grow"></span>
    {#if !inShell}
      <div data-shell class="cell">
        <button data-shell type="button" class="cellbtn" onclick={() => filters.setView('notifications')}>{t('strip.notifications')}{#if unreadNotifications}<span data-shell class="badge unread">{unreadNotifications}</span>{/if}</button>
      </div>
      <div data-shell class="cell">
        <button data-shell type="button" class="cellbtn" onclick={() => filters.setView('settings')}><span data-shell aria-hidden="true">⚙</span>{t('strip.settings')}</button>
      </div>
      {#if keyboardTools}<div data-shell class="cell">{@render keyboardTools()}</div>{/if}
      <div data-shell class="cell end more">
        <button data-shell type="button" class="cellbtn more-trigger" bind:this={moreTrigger} aria-expanded={moreOpen} aria-controls="more-pop" onclick={() => (moreOpen = !moreOpen)}>{t('strip.more')}</button>
        {#if moreOpen}
          <div data-shell id="more-pop" class="more-pop" use:closeOnFocusOut={{ trigger: () => moreTrigger, close: () => (moreOpen = false) }}>
            <button data-shell type="button" onclick={() => { moreTrigger?.focus(); moreOpen = false; onupdates(); }}>{t('strip.checkUpdates')}</button>
            <button data-shell type="button" onclick={() => { moreTrigger?.focus(); moreOpen = false; onfeedback(); }}>{t('strip.sendFeedback')}</button>
            <hr data-shell />
            <a data-shell href="#faq" onclick={() => (moreOpen = false)}>{t('strip.faq')}</a>
            {@render projectLinkAnchors()}
          </div>
        {/if}
      </div>
    {:else}
      {#if activity?.current}
        <div data-shell class="cell act {activity.current.tone}" role="status" aria-live="polite">
          <span data-shell class="dot" aria-hidden="true"></span>
          <span data-shell class="act-label" title={activity.current.label}>{activity.current.label}</span>
        </div>
      {/if}
      <div data-shell class="cell end">
        <span data-shell class="fit-3">WFM</span>
        {#if listing.wfmStatus && !listing.wfmStatus.unlocked}
          <!-- The state, then the click that moves it on, so the step is visible. -->
          <span data-shell class="wfm-state">{listing.wfmStatus.logged_in ? t('strip.locked') : t('strip.signedOut')}</span>
          <button data-shell type="button" class="wfm-auth" data-testid="wfm-auth-link" onclick={() => onauth(listing.wfmStatus?.logged_in ? 'needs_unlock' : 'needs_login')}>{listing.wfmStatus.logged_in ? t('strip.unlock') : t('strip.signIn')} <span data-shell aria-hidden="true">→</span></button>
        {:else if presence?.status?.signedIn}
          <PresenceMenu {presence} onsettings={onpresencesettings} />
        {:else}
          <b data-shell>{wfmLabel}</b>
        {/if}
        {#if listing.ordersSummary}<span data-shell class="fit-2">· {listing.ordersSummary.live} live</span>{/if}
      </div>
    {/if}
  </header>
