<script lang="ts">
  import { t } from '../../i18n/i18n.svelte';
  import type { OverlaySettings } from '../../contracts/data';

  let { scanning, scanText, autoScanOn, overlay, wfmStatus, onscan, onautoscan, onoverlay, onconnect, onsignout, ondesktop }: {
    scanning: boolean;
    scanText: string;
    autoScanOn: boolean | null;
    overlay: OverlaySettings | null;
    wfmStatus: { logged_in: boolean; unlocked: boolean } | null;
    onscan: () => void;
    onautoscan: (on: boolean) => void;
    onoverlay: (on: boolean) => void;
    onconnect: () => void;
    onsignout: () => void;
    ondesktop: () => void;
  } = $props();
</script>

<section class="gm-section" aria-labelledby="gm-setup-title">
  <header class="gm-head"><h2 id="gm-setup-title">{t('gaming.setup')}</h2></header>
  <ul class="gm-list" data-pad-scroll>
    <li>
      <button type="button" class="gm-row" data-pad-default disabled={scanning} onclick={onscan}>
        <span class="gm-main"><span class="gm-name">{t('gaming.scanNow')}</span><span class="gm-sub">{scanText}</span></span>
        <kbd class="gm-key">Y</kbd>
      </button>
    </li>
    {#if autoScanOn != null}
      <li>
        <button type="button" class="gm-row" role="switch" aria-checked={autoScanOn} onclick={() => onautoscan(!autoScanOn)}>
          <span class="gm-main"><span class="gm-name">{t('gaming.autoScan')}</span><span class="gm-sub">{t('gaming.autoScanHelp')}</span></span>
          <span class="gm-switch" class:on={autoScanOn}>{autoScanOn ? t('gaming.on') : t('gaming.off')}</span>
        </button>
      </li>
    {/if}
    {#if overlay}
      <li>
        <button type="button" class="gm-row" role="switch" aria-checked={overlay.enabled} onclick={() => onoverlay(!overlay.enabled)}>
          <span class="gm-main"><span class="gm-name">{t('gaming.overlay')}</span><span class="gm-sub">{t('gaming.overlayHelp')}</span></span>
          <span class="gm-switch" class:on={overlay.enabled}>{overlay.enabled ? t('gaming.on') : t('gaming.off')}</span>
        </button>
      </li>
    {/if}
    <li>
      {#if wfmStatus?.logged_in}
        <button type="button" class="gm-row" onclick={wfmStatus.unlocked ? onsignout : onconnect}>
          <span class="gm-main"><span class="gm-name">{t('gaming.account')}</span><span class="gm-sub">{wfmStatus.unlocked ? t('gaming.wfmSignedIn') : t('gaming.wfmLocked')}</span></span>
          <span class="gm-verb">{wfmStatus.unlocked ? t('gaming.signOut') : t('gaming.connect')}</span>
        </button>
      {:else}
        <button type="button" class="gm-row" onclick={onconnect}>
          <span class="gm-main"><span class="gm-name">{t('gaming.account')}</span><span class="gm-sub">{t('gaming.accountHelp')}</span></span>
          <span class="gm-verb">{t('gaming.connect')}</span>
        </button>
      {/if}
    </li>
    <li>
      <button type="button" class="gm-row" onclick={ondesktop}>
        <span class="gm-main"><span class="gm-name">{t('gaming.desktopLayout')}</span><span class="gm-sub">{t('gaming.desktopLayoutHelp')}</span></span>
      </button>
    </li>
  </ul>
  <p class="gm-tip muted">{t('gaming.pasteTip')}</p>
</section>
