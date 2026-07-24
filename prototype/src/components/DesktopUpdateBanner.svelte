<script lang="ts">
  // C5 desktop auto-update banner. Self-contained: registers its own
  // update-available listener + pulls the stored status on mount, and owns
  // every bit of state the install/restart flow needs. Desktop-only by
  // construction (see lib/desktop-update.ts) — App.svelte only mounts this
  // inside `{#if isDesktop}`, so its onMount never runs in the browser build.
  import { onMount } from 'svelte';
  import {
    updateStatus as fetchUpdateStatus, installUpdate, restartApp, onUpdateAvailable,
    type UpdateStatus,
  } from '../lib/desktop-update';
  import { humanError } from '../lib/errors';

  let updateInfo = $state<UpdateStatus | null>(null);
  let updateInstalling = $state(false);
  let updateInstalled = $state(false);
  let updateError = $state<string | null>(null);
  let updateDismissed = $state(false);

  onMount(() => {
    // C5 update-available handshake: listen for the push (the Rust launch
    // check may still be in flight) AND pull the stored status (its emit may
    // have beaten this listener). Best-effort — a failure here must never
    // disturb boot, and "no update" needs no UI at all.
    onUpdateAvailable((s) => {
      if (s.available) updateInfo = s;
    });
    (async () => {
      try {
        const s = await fetchUpdateStatus();
        if (s.available) updateInfo = s;
      } catch (e) {
        console.error('update status read failed', e);
      }
    })();
  });

  // Explicit confirmation is THE gate: install_update rejects on a download
  // failure or a bad bundle signature, which lands in the banner while the
  // running app stays intact (and the update stays retryable).
  async function installUpdateNow() {
    updateError = null;
    updateInstalling = true;
    try {
      await installUpdate();
      updateInstalled = true;
    } catch (e) {
      updateError = humanError(e);
    } finally {
      updateInstalling = false;
    }
  }

  async function restartToApply() {
    try {
      await restartApp();
    } catch (e) {
      updateError = humanError(e);
    }
  }
</script>

{#if updateInfo && !updateDismissed}
  <div class="card warn-banner general-banner" role="status" data-testid="update-banner">
    <div class="gb-body">
      {#if updateInstalled}
        <strong>Update installed.</strong> TennoWorth v{updateInfo.version} takes over
        the next time the app starts.
      {:else}
        <strong>Update available:</strong> TennoWorth v{updateInfo.version}
        (you have v{updateInfo.current_version}). Nothing downloads until you install.
        {#if updateError}<br />{updateError}{/if}
      {/if}
    </div>
    <div class="gb-actions">
      {#if updateInstalled}
        <button onclick={restartToApply}>Restart now</button>
      {:else}
        <button onclick={installUpdateNow} disabled={updateInstalling}>
          {updateInstalling ? 'Installing…' : 'Install update'}
        </button>
      {/if}
      <button class="gb-dismiss" aria-label="Dismiss" onclick={() => (updateDismissed = true)}>×</button>
    </div>
  </div>
{/if}

<style>
  /* Duplicated from App.svelte's shared banner styles — Svelte scopes CSS
     per-component, and this codebase's existing extracted components
     (e.g. MarketBrowser's `.card`) already re-declare shared visual classes
     rather than promoting them to global CSS. Keep in sync by eye if the
     shared banner look changes. */
  .card {
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 14px 16px;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .warn-banner {
    border-left: 3px solid var(--bad);
    padding: 10px 14px;
    font-size: 13px;
    color: var(--fg);
    line-height: 1.5;
  }
  .general-banner {
    flex-direction: row;
    align-items: flex-start;
    justify-content: space-between;
    gap: 12px;
  }
  .general-banner .gb-body { min-width: 0; }
  .general-banner .gb-actions {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-shrink: 0;
  }
  .general-banner .gb-dismiss {
    background: transparent;
    border: none;
    color: var(--muted);
    font-size: 16px;
    line-height: 1;
    cursor: pointer;
    padding: 3px;
  }
  .general-banner .gb-dismiss:hover { color: var(--fg); }
</style>
