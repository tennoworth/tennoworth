<script lang="ts">
  import { onMount } from 'svelte';
  import { t } from '../../i18n/i18n.svelte';
  import { startNavigator } from './navigator';
  import GamingSell from './GamingSell.svelte';
  import GamingRewards from './GamingRewards.svelte';
  import GamingOrders from './GamingOrders.svelte';
  import GamingSetup from './GamingSetup.svelte';
  import type { SellRow } from '../../contracts/selling';
  import type { OverlaySettings, OverlayStatus, TradeAllowance } from '../../contracts/data';
  import type { OrdersInput } from '../orders/controller.svelte';
  import type { AutoScanController } from '../inventory/auto-scan.svelte';

  type Section = 'sell' | 'rewards' | 'orders' | 'setup';
  const SECTIONS: Section[] = ['sell', 'rewards', 'orders', 'setup'];

  let { ordersIssues, results, ready, hasInventory, canList, blockReason, actionLabel, oncheck, tradeAllowance, scanning, scanText, wfmStatus, autoScan, overlayPort, ordersInput, onlist, onscan, onauth, onsignout, ondesktop }: {
    results: SellRow[];
    ready: boolean;
    hasInventory: boolean;
    canList: boolean;
    /** Listings that need a fix; X offers "Fix all prices" only when some do. */
    ordersIssues: number;
    blockReason: string | null;
    actionLabel: string | null;
    oncheck: () => void;
    tradeAllowance: TradeAllowance | null;
    scanning: boolean;
    scanText: string;
    wfmStatus: { logged_in: boolean; unlocked: boolean } | null;
    autoScan: AutoScanController;
    overlayPort: {
      getOverlaySettings(): Promise<OverlaySettings>;
      updateOverlaySettings(settings: OverlaySettings): Promise<OverlaySettings>;
      overlayStatus(): Promise<OverlayStatus>;
    };
    ordersInput: Omit<OrdersInput, 'active'>;
    onlist: (rows?: SellRow | SellRow[]) => void;
    onscan: () => void;
    onauth: (code: 'needs_login' | 'needs_unlock') => void;
    onsignout: () => void;
    ondesktop: () => void;
  } = $props();

  let section = $state<Section>('sell');
  let root = $state<HTMLElement>();
  let sell = $state<{ secondary(): void }>();
  let rewards = $state<{ secondary(): void }>();
  let orders = $state<{ secondary(): void }>();

  let overlay = $state<OverlaySettings | null>(null);
  let overlayStatus = $state<OverlayStatus | null>(null);
  async function refreshOverlay() {
    try {
      [overlay, overlayStatus] = await Promise.all([overlayPort.getOverlaySettings(), overlayPort.overlayStatus()]);
    } catch (error) { console.error(error); }
  }
  async function setOverlay(enabled: boolean) {
    if (!overlay) return;
    try {
      overlay = await overlayPort.updateOverlaySettings({ ...overlay, enabled });
      overlayStatus = await overlayPort.overlayStatus();
    } catch (error) { console.error(error); }
  }
  const connect = () => onauth(wfmStatus?.logged_in ? 'needs_unlock' : 'needs_login');

  function show(next: Section) {
    section = next;
    void refreshOverlay();
    // The new section's first control takes focus, so A works at once.
    queueMicrotask(() => {
      const pane = root?.querySelector<HTMLElement>('.gm-pane:not([hidden])');
      const first = pane?.querySelector<HTMLElement>('[data-pad-default]:not([disabled])') ?? pane?.querySelector<HTMLElement>('button:not([disabled]), [tabindex="0"]');
      first?.focus({ preventScroll: true });
    });
  }
  const step = (by: number) => show(SECTIONS[(SECTIONS.indexOf(section) + by + SECTIONS.length) % SECTIONS.length]);

  onMount(() => {
    void refreshOverlay();
    const refresh = () => void refreshOverlay();
    window.addEventListener('focus', refresh);
    const stop = startNavigator(root!, {
      back: () => { if (section !== 'sell') show('sell'); },
      prev: () => step(-1),
      next: () => step(1),
      menu: () => show('setup'),
      scan: () => { if (!scanning) onscan(); },
      secondary: () => ({ sell, rewards, orders, setup: undefined })[section]?.secondary(),
    });
    show('sell');
    return () => { stop(); window.removeEventListener('focus', refresh); };
  });

  let secondaryLabel = $derived(({ sell: !hasInventory ? '' : canList ? t('gaming.listAll') : (actionLabel ?? t('sell.checkListings')), rewards: t('gaming.refresh'), orders: ordersIssues ? t('gaming.fixAll') : '', setup: '' })[section]);
  let wfmText = $derived(!wfmStatus?.logged_in ? t('gaming.wfmSignedOut') : wfmStatus.unlocked ? t('gaming.wfmSignedIn') : t('gaming.wfmLocked'));
  const label = (s: Section) => t(`gaming.${s}` as 'gaming.sell');
</script>

<div class="gaming" bind:this={root}>
  <header class="gm-rail">
    <span class="gm-brand" aria-hidden="true"></span>
    <kbd class="gm-key small">LB</kbd>
    <div class="gm-tabs" role="tablist" aria-label={t('gaming.sections')}>
      {#each SECTIONS as s (s)}
        <button type="button" role="tab" class="gm-tab" aria-selected={section === s} tabindex={-1} onclick={() => show(s)}>{label(s)}</button>
      {/each}
    </div>
    <kbd class="gm-key small">RB</kbd>
    <p class="gm-status" role="status">{scanning ? t('gaming.scanning') : scanText} · {wfmText}</p>
  </header>
  <main class="gm-body">
    <div class="gm-pane" hidden={section !== 'sell'}><GamingSell bind:this={sell} {results} {ready} {hasInventory} {canList} {blockReason} {actionLabel} {tradeAllowance} {onlist} {oncheck} /></div>
    <div class="gm-pane" hidden={section !== 'rewards'}><GamingRewards bind:this={rewards} settings={overlay} status={overlayStatus} onenable={() => void setOverlay(true)} /></div>
    <div class="gm-pane" hidden={section !== 'orders'}><GamingOrders bind:this={orders} input={ordersInput} active={section === 'orders'} {wfmStatus} onconnect={connect} /></div>
    <div class="gm-pane" hidden={section !== 'setup'}>
      <GamingSetup {scanning} {scanText} autoScanOn={autoScan.settings?.enabled ?? null} {overlay} {wfmStatus}
        {onscan} onautoscan={(on) => { if (autoScan.settings) void autoScan.save({ ...autoScan.settings, enabled: on }); }}
        onoverlay={(on) => void setOverlay(on)} onconnect={connect} {onsignout} {ondesktop} />
    </div>
  </main>
  <footer class="gm-legend" aria-label={t('gaming.sections')}>
    <span><kbd class="gm-key">A</kbd>{t('gaming.legendSelect')}</span>
    <span><kbd class="gm-key">B</kbd>{t('gaming.legendBack')}</span>
    {#if secondaryLabel}<span><kbd class="gm-key">X</kbd>{secondaryLabel}</span>{/if}
    <span><kbd class="gm-key">Y</kbd>{t('gaming.legendScan')}</span>
    <span><kbd class="gm-key">LB</kbd><kbd class="gm-key">RB</kbd>{t('gaming.legendSection')}</span>
  </footer>
</div>

<style>
  .gaming {
    display: grid;
    grid-template-rows: auto 1fr auto;
    height: 100vh;
    background: var(--bg);
    color: var(--fg);
    font-family: var(--font-body);
  }
  .gm-rail {
    display: flex;
    align-items: center;
    gap: var(--s3);
    padding: 0 var(--s4);
    min-height: 3.25rem;
    background: var(--rail-bg);
    color: var(--rail-fg);
    border-bottom: 2px solid var(--rail-edge);
  }
  .gm-brand { width: 0.75rem; height: 0.75rem; background: var(--rail-fg); flex: none; }
  .gm-tabs { display: flex; gap: var(--s1); }
  .gm-tab {
    min-height: 2.75rem;
    padding: 0 var(--s4);
    border: 1px solid transparent;
    background: transparent;
    color: var(--rail-muted);
    font: inherit;
    font-family: var(--font-ui);
    font-size: var(--text-section);
    cursor: pointer;
  }
  .gm-tab[aria-selected='true'] { color: var(--rail-fg); border-color: var(--rail-fg); }
  .gm-status { margin: 0 0 0 auto; color: var(--rail-muted); font-size: var(--text-caption); text-align: right; }
  .gm-body { overflow: hidden; padding: var(--s4) var(--s5); min-height: 0; }
  .gm-pane { height: 100%; }
  .gm-pane[hidden] { display: none; }
  .gm-legend {
    display: flex;
    flex-wrap: wrap;
    gap: var(--s5);
    padding: var(--s2) var(--s5);
    border-top: 1px solid var(--border);
    background: var(--panel);
    font-family: var(--font-ui);
    font-size: var(--text-control);
  }
  .gm-legend span { display: inline-flex; align-items: center; gap: var(--s2); }

  .gaming :global(.gm-key) {
    display: inline-grid;
    place-items: center;
    min-width: 1.75rem;
    height: 1.75rem;
    padding: 0 var(--s1);
    border: 1px solid currentColor;
    font-family: var(--font-mono);
    font-size: var(--text-caption);
    font-weight: 600;
  }
  .gaming :global(.gm-key.small) { min-width: 1.5rem; height: 1.5rem; }
  .gaming :global(.gm-section) { display: flex; flex-direction: column; gap: var(--s3); height: 100%; min-height: 0; }
  .gaming :global(.gm-section > .gm-list) { flex: 1; min-height: 0; }
  .gaming :global(.gm-notice) { display: flex; flex-wrap: wrap; align-items: center; gap: var(--s2) var(--s4); padding: var(--s3) var(--s4); border: 1px solid var(--border); box-shadow: inset 3px 0 0 var(--warn); background: var(--panel); }
  .gaming :global(.gm-notice p) { margin: 0; flex: 1 1 20rem; }
  .gaming :global(.gm-head) { display: flex; flex-wrap: wrap; align-items: center; gap: var(--s2) var(--s4); }
  .gaming :global(.gm-head h2) { margin: 0; font-family: var(--font-ui); font-size: var(--text-heading); }
  .gaming :global(.gm-head p) { margin: 0; }
  .gaming :global(.gm-head .gm-action) { margin-left: auto; }
  .gaming :global(.gm-list) { list-style: none; margin: 0; padding: 0 var(--s1) var(--s4) 0; overflow-y: auto; display: grid; gap: var(--s2); align-content: start; }
  .gaming :global(.gm-row) {
    display: flex;
    align-items: center;
    gap: var(--s4);
    width: 100%;
    min-height: 3.5rem;
    padding: var(--s2) var(--s4) var(--s2) var(--s5);
    border: 1px solid var(--border);
    background: var(--panel);
    color: var(--fg);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .gaming :global(.gm-row:disabled) { cursor: default; border-style: dotted; }
  .gaming :global(.gm-row.issue) { box-shadow: inset 3px 0 0 var(--warn); }
  .gaming :global(.gm-main) { display: grid; gap: 0.125rem; flex: 1; min-width: 0; }
  .gaming :global(.gm-name) { font-weight: 600; overflow-wrap: anywhere; }
  .gaming :global(.gm-sub) { color: var(--muted); font-size: var(--text-caption); }
  .gaming :global(.gm-figs) { display: grid; justify-items: end; gap: 0.125rem; }
  .gaming :global(.gm-big) { font-size: var(--text-metric-lg); font-weight: 600; }
  .gaming :global(.gm-big .unit) { font-size: var(--text-caption); font-weight: 400; color: var(--muted); margin-left: 0.125rem; }
  .gaming :global(.gm-verb), .gaming :global(.gm-switch) {
    min-width: 4.5rem;
    padding: var(--s1) var(--s3);
    border: 1px solid var(--border);
    font-family: var(--font-ui);
    text-align: center;
  }
  .gaming :global(.gm-switch.on) { background: var(--ink-bar); color: var(--on-ink); border-color: var(--ink-bar); }
  .gaming :global(.gm-action) {
    display: inline-flex;
    align-items: center;
    gap: var(--s2);
    min-height: 3rem;
    padding: 0 var(--s4);
    border: 1px solid var(--border);
    background: var(--panel);
    color: var(--fg);
    font: inherit;
    font-family: var(--font-ui);
    cursor: pointer;
  }
  .gaming :global(.gm-action.primary) { background: var(--ink-bar); color: var(--on-ink); border-color: var(--ink-bar); }
  .gaming :global(.gm-empty) { display: grid; gap: var(--s3); justify-items: start; align-content: start; max-width: 40rem; padding: var(--s4) 0; }
  .gaming :global(.gm-empty p) { margin: 0; }
  .gaming :global(.gm-cards) { list-style: none; margin: 0; padding: 0; display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: var(--s3); align-content: start; }
  .gaming :global(.gm-card) { display: grid; gap: var(--s1); align-content: start; min-height: 9rem; padding: var(--s4); border: 1px solid var(--border); background: var(--panel); }
  .gaming :global(.gm-card.best) { border-width: 2px; border-color: var(--fg); }
  .gaming :global(.gm-tags) { display: flex; flex-wrap: wrap; gap: var(--s1); }
  .gaming :global(.gm-tip) { margin: 0; font-size: var(--text-caption); }

  /* One focus, the ink inversion with the row cursor's square, however it
     arrived: a pad moves focus from script, which :focus-visible may not mark. */
  .gaming :global(:is(.gm-row, .gm-card, .gm-action, .gm-tab):focus) {
    outline: none;
    background: var(--ink-bar);
    color: var(--on-ink);
    border-color: var(--ink-bar);
  }
  .gaming :global(:is(.gm-row, .gm-card):focus) { box-shadow: inset 0.5rem 0 0 -0.125rem var(--on-ink); }
  .gaming :global(:is(.gm-row, .gm-card, .gm-action):focus :is(.gm-sub, .unit, .muted)) { color: var(--on-ink-muted); }
  .gaming :global(:is(.gm-row, .gm-card):focus :is(.gm-verb, .gm-switch)) { border-color: var(--on-ink); }
</style>
