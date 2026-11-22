<script lang="ts">
  import { onMount } from 'svelte';
  import { t } from '../../i18n/i18n.svelte';
  import type { OverlaySettings, OverlayStatus, RelicOverlayResult } from '../../contracts/data';
  import { useDesktopServices } from '../shell/desktop-context';

  let { settings, status, onenable }: {
    settings: OverlaySettings | null;
    status: OverlayStatus | null;
    onenable: () => void;
  } = $props();
  const { currentOverlayResult } = useDesktopServices();

  let result = $state<RelicOverlayResult | null>(null);
  export async function secondary() {
    try { result = (await currentOverlayResult()) ?? result; } catch (error) { console.error(error); }
  }
  // The overlay's result goes to the overlay itself; this reads the latest
  // when the section opens and whenever the player comes back to the app.
  onMount(() => {
    void secondary();
    const refresh = () => void secondary();
    window.addEventListener('focus', refresh);
    return () => window.removeEventListener('focus', refresh);
  });

  let statusText = $derived(!status ? '' : status.state === 'error' ? t('gaming.overlayError', { message: status.message ?? '' }) : status.state === 'waiting-for-game' ? t('gaming.overlayWaiting') : t('gaming.overlayWatching'));
</script>

<section class="gm-section" aria-labelledby="gm-rewards-title">
  <header class="gm-head">
    <h2 id="gm-rewards-title">{t('gaming.lastReward')}</h2>
    {#if settings?.enabled}<p class="muted" role="status">{statusText}</p>{/if}
  </header>
  {#if settings && !settings.enabled}
    <div class="gm-empty">
      <strong>{t('gaming.overlayOff')}</strong>
      <p>{t('gaming.overlayHelp')}</p>
      <button type="button" class="gm-action primary" data-pad-default onclick={onenable}>{t('gaming.turnOnOverlay')}</button>
    </div>
  {/if}
  {#if result?.slots.length}
    <ul class="gm-cards">
      {#each result.slots as slot (slot.index)}
        {@const plat = slot.livePlatinum ?? slot.cachedPlatinum}
        <li class="gm-card" class:best={slot.bestPlatinum || slot.bestDucats}>
          <span class="gm-name">{slot.name ?? t('gaming.unread')}</span>
          <span class="gm-big mono">{plat != null ? plat : '-'}<span class="unit">p</span></span>
          {#if slot.ducats != null}<span class="gm-sub">{t('gaming.ducats', { count: slot.ducats })}</span>{/if}
          {#if slot.owned != null}<span class="gm-sub">{t('gaming.owned', { count: slot.owned })}</span>{/if}
          <span class="gm-tags">
            {#if slot.bestPlatinum}<span class="tag peak">{t('gaming.bestPlat')}</span>{/if}
            {#if slot.bestDucats}<span class="tag deal">{t('gaming.bestDucats')}</span>{/if}
          </span>
        </li>
      {/each}
    </ul>
  {:else if settings?.enabled}
    <div class="gm-empty"><p>{t('gaming.noReward')}</p></div>
  {/if}
</section>
