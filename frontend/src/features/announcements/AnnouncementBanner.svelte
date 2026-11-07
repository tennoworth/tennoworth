<script lang="ts">
  import type { Announcement } from '../../contracts/generated/desktop';
  import { t } from '../../i18n/i18n.svelte';
  let { message, saving = false, error = '', ondismiss }: {
    message: Announcement; saving?: boolean; error?: string; ondismiss?: () => void;
  } = $props();
</script>

<section class="ui-notice general-banner market-announcement" data-tone="warn" aria-label={message.title}>
  <div class="gb-body ui-stack">
    <strong>{message.title}</strong>
    <p>{message.body}</p>
    {#if error}<p class="bad" role="alert">{error}</p>{/if}
  </div>
  <div class="gb-actions">
    <a class="btn xs" href={message.source_url} target="_blank" rel="noopener noreferrer">{t('note.source')}</a>
    <button type="button" class="btn ghost xs" aria-label={t('announce.dismissNamed', { title: message.title })} disabled={saving || !ondismiss} onclick={ondismiss}>{t('announce.dismiss')}</button>
  </div>
</section>

<style>
  .market-announcement { display: flex; margin-bottom: var(--s4); }
  .gb-body { gap: var(--s1); }
  .gb-body p { margin: 0; white-space: pre-line; }
</style>
