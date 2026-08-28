<script lang="ts">
  import { onMount } from 'svelte';
  import { listenForTauriEvent } from '../lib/desktop-update';
  import { resolveInvoke } from '../lib/transport';
  import type { RelicOverlayResult } from '../lib/types';

  let result = $state<RelicOverlayResult | null>(null);

  onMount(() => {
    window.__TENNOWORTH_RELIC_OVERLAY_UPDATE__ = (next) => { result = next; };
    window.__TENNOWORTH_RELIC_OVERLAY_HIDE__ = () => { result = null; };
    resolveInvoke()<RelicOverlayResult | null>('current_overlay_result')
      .then((current) => { if (current) result = current; })
      .catch(() => {});
    const unlistenUpdate = listenForTauriEvent<RelicOverlayResult>('relic-overlay:update', (payload) => {
      result = payload;
    });
    const unlistenHide = listenForTauriEvent('relic-overlay:hide', () => {
      result = null;
    });
    return () => {
      unlistenUpdate();
      unlistenHide();
      delete window.__TENNOWORTH_RELIC_OVERLAY_UPDATE__;
      delete window.__TENNOWORTH_RELIC_OVERLAY_HIDE__;
    };
  });

  const price = (slot: RelicOverlayResult['slots'][number]) =>
    slot.livePlatinum ?? slot.cachedPlatinum;
</script>

<svelte:head><title>TennoWorth relic overlay</title></svelte:head>

<div class="overlay" aria-live="polite">
  {#if result}
    {#each result.slots as slot (slot.index)}
      <article
        class="reward"
        class:best={slot.bestPlatinum}
        class:uncertain={slot.confidence < 0.9}
        style:left={`${slot.box.x * 100}%`}
        style:top={`${slot.box.y * 100}%`}
        style:width={`${slot.box.width * 100}%`}
      >
        <div class="inner" style:transform={`scale(${result.scale})`}>
          {#if slot.bestPlatinum}<span class="best-flag">BEST PLAT</span>{/if}
          {#if slot.bestDucats && !slot.bestPlatinum}<span class="ducat-flag">BEST DUCATS</span>{/if}
          <strong class="name">{slot.name ?? slot.rawText}</strong>
          <div class="facts">
            <b class="plat">{price(slot) == null ? '—' : `${price(slot)}p`}</b>
            <span>{slot.ducats == null ? '—d' : `${slot.ducats}d`}</span>
            <span>{slot.owned == null ? 'own —' : `own ${slot.owned}`}</span>
            <span class="source">{slot.livePlatinum == null ? 'cached' : 'live'}</span>
          </div>
          {#if slot.confidence < 0.9}
            <span class="confidence">check name · {Math.round(slot.confidence * 100)}%</span>
          {/if}
        </div>
      </article>
    {/each}
  {/if}
</div>

<style>
  :global(html), :global(body), :global(#app) { width: 100%; height: 100%; margin: 0; background: transparent !important; overflow: hidden; }
  .overlay { position: fixed; inset: 0; pointer-events: none; font-family: var(--font-ui, system-ui, sans-serif); color: #edf2f4; }
  .reward { position: absolute; box-sizing: border-box; padding: 0 8px; transform: translateY(8px); }
  .inner { position: relative; max-width: 250px; min-height: 66px; margin: auto; padding: 9px 11px; border: 1px solid #66808f; border-radius: 7px; background: #091117e8; box-shadow: 0 6px 22px #000a; backdrop-filter: blur(5px); transform-origin: top center; }
  .reward.best .inner { border-color: #e6b85c; box-shadow: 0 0 22px #e6b85c55, 0 6px 22px #000a; }
  .reward.uncertain .inner { border-color: #ee8f70; }
  .best-flag, .ducat-flag { position: absolute; right: 7px; top: -10px; padding: 2px 7px; border-radius: 4px; background: #e6b85c; color: #171108; font: 700 9px/1.4 ui-monospace, monospace; letter-spacing: .08em; }
  .ducat-flag { background: #75cbd0; color: #071517; }
  .name { display: block; padding-right: 44px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; font-size: 12px; text-shadow: 0 1px 3px #000; }
  .facts { display: flex; align-items: baseline; gap: 9px; color: #b6c1c8; font: 10px/1.5 ui-monospace, monospace; }
  .plat { color: #e6b85c; font-size: 22px; }
  .source { color: #62d8d0; margin-left: auto; }
  .confidence { display: block; color: #ee8f70; font: 9px/1.25 ui-monospace, monospace; }
</style>
