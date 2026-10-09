<script lang="ts">
  // Desktop update banner. Desktop-only by construction (see
  // adapters/desktop-update.ts) - only the desktop shell mounts it. State lives
  // in the shell's UpdateController, shared with Settings → Updates.
  import { LATEST_RELEASE_URL, type UpdateController } from './update-controller.svelte';

  let { updates }: { updates: UpdateController } = $props();
</script>

{#if (updates.announced || updates.info?.available) && !updates.dismissed}
  <div class="card ui-panel warn-banner general-banner" role="status" data-testid="update-banner">
    <div class="gb-body">
      {#if updates.installed}
        <strong>Update installed.</strong> TennoWorth v{updates.info?.version} takes over the next time the app starts.
      {:else if updates.installing}
        <strong>Installing update…</strong> Keep TennoWorth open until installation finishes.
      {:else if updates.checking}
        <strong>Checking for updates…</strong>
      {:else if updates.info?.available}
        <strong>Update available:</strong> TennoWorth v{updates.info.version}
        (you have v{updates.info.current_version}). Nothing downloads until you install.
      {:else if !updates.error}
        {#if updates.info?.support === 'appimage_required'}
          This install can’t update itself. Download and run the TennoWorth AppImage to receive updates.
        {:else if updates.info?.support === 'disabled_test_build'}
          Updates are disabled in this test build.
        {:else if updates.info?.checked}
          No update was offered · v{updates.info.current_version}. If you’re offline, reconnect and check again.
        {:else}
          Couldn’t check for updates. Check your connection and try again.
        {/if}
      {/if}
      {#if updates.needsManualInstall}
        <p data-testid="update-manual-install">
          This update can't be installed from inside the app. Download it once from the
          <a href={LATEST_RELEASE_URL} target="_blank" rel="noopener noreferrer">latest release</a>
          and run it - your settings and data are kept, and updates install normally again after that.
        </p>
      {/if}
      {#if updates.error}<p>{updates.error}</p>{/if}
    </div>
    <div class="gb-actions">
      {#if updates.installed}
        <button class="btn primary" onclick={() => updates.restart()}>Restart now</button>
      {:else if updates.info?.available}
        <button class="btn primary" onclick={() => updates.install()} disabled={updates.installing || updates.checking}>
          {updates.installing ? 'Installing…' : 'Install update'}
        </button>
      {:else}
        <button class="btn" onclick={() => updates.check({ announce: true })} disabled={updates.checking}>{updates.checking ? 'Checking…' : 'Check again'}</button>
      {/if}
      <button class="gb-dismiss" aria-label="Dismiss update notice" onclick={() => (updates.dismissed = true)} disabled={updates.installing}>×</button>
    </div>
  </div>
{/if}

<style>
  @media (max-width: 35rem) {
    .general-banner .gb-actions { width: 100%; justify-content: flex-end; }
  }
</style>
