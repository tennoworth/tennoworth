<script lang="ts">
  import type { Announcement } from '../../contracts/generated/desktop';
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
    <a class="btn xs" href={message.source_url} target="_blank" rel="noopener noreferrer">Source announcement ↗</a>
    <button type="button" class="btn ghost xs" aria-label="Dismiss {message.title}" disabled={saving || !ondismiss} onclick={ondismiss}>Dismiss</button>
  </div>
</section>

<style>
  .market-announcement { display: flex; margin-bottom: var(--s4); }
  .gb-body { gap: var(--s1); }
  .gb-body p { margin: 0; white-space: pre-line; }
</style>
