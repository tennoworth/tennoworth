<script lang="ts">
  // Desktop update banner. Desktop-only by construction (see
  // adapters/desktop-update.ts) - only the desktop shell mounts it. State lives
  // in the shell's UpdateController, shared with Settings → Updates.
  import { LATEST_RELEASE_URL, type UpdateController } from './update-controller.svelte';
  import { t } from '../../i18n/i18n.svelte';
  import Rich from '../../ui/Rich.svelte';

  let { updates }: { updates: UpdateController } = $props();
</script>

{#if (updates.announced || updates.info?.available) && !updates.dismissed}
  <div class="card ui-panel warn-banner general-banner" role="status" data-testid="update-banner">
    <div class="gb-body">
      {#if updates.installed}
        <Rich text={t('update.installed', { version: updates.info?.version ?? '' })} />
      {:else if updates.installing}
        <Rich text={t('update.installing')} />
      {:else if updates.checking}
        <Rich text={t('update.checking')} />
      {:else if updates.info?.available}
        <Rich text={t('update.available', { version: updates.info.version ?? '', current: updates.info.current_version })} />
      {:else if !updates.error}
        {#if updates.info?.support === 'appimage_required'}
          {t('settings.update.appimage')}
        {:else if updates.info?.support === 'disabled_test_build'}
          {t('settings.update.disabled')}
        {:else if updates.info?.checked}
          {t('update.none', { current: updates.info.current_version })}
        {:else}
          {t('update.failed')}
        {/if}
      {/if}
      {#if updates.needsManualInstall}
        <p data-testid="update-manual-install">
          {t('settings.manualBefore')}<a href={LATEST_RELEASE_URL} target="_blank" rel="noopener noreferrer">{t('settings.latestRelease')}</a>{t('settings.manualAfter')}
        </p>
      {/if}
      {#if updates.error}<p>{updates.error}</p>{/if}
    </div>
    <div class="gb-actions">
      {#if updates.installed}
        <button class="btn primary" onclick={() => updates.restart()}>{t('settings.restart')}</button>
      {:else if updates.info?.available}
        <button class="btn primary" onclick={() => updates.install()} disabled={updates.installing || updates.checking}>
          {t(updates.installing ? 'settings.installing' : 'settings.install')}
        </button>
      {:else}
        <button class="btn" onclick={() => updates.check({ announce: true })} disabled={updates.checking}>{t(updates.checking ? 'settings.checking' : 'update.checkAgain')}</button>
      {/if}
      <button class="gb-dismiss" aria-label={t('update.dismiss')} onclick={() => (updates.dismissed = true)} disabled={updates.installing}>×</button>
    </div>
  </div>
{/if}

<style>
  @media (max-width: 35rem) {
    .general-banner .gb-actions { width: 100%; justify-content: flex-end; }
  }
</style>
