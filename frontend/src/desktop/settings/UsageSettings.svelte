<script lang="ts">
  import { t } from '../../i18n/i18n.svelte';
  import { onMount } from 'svelte';
  import type { DesktopCapabilities } from '../../contracts/desktop';
  import type { UsagePreferences } from '../../contracts/usage';
  let { transport }: { transport: DesktopCapabilities } = $props();
  let preference = $state<UsagePreferences | null>(null);
  let saving = $state(false);
  let error = $state('');
  onMount(() => { void transport.getUsagePreferences().then(value => { preference=value; }).catch(() => { error=t('usage.loadFailed'); }); });
  async function change(enabled: boolean) {
    saving=true; error='';
    try { preference=await transport.setUsagePreferences(enabled); }
    catch {
      try { preference=await transport.getUsagePreferences(); }
      catch { preference=null; }
      error=t('privacy.saveFailed');
    }
    finally { saving=false; }
  }
</script>
<!-- A group inside Settings' "This app" panel: set once, so the full privacy
     terms sit one click away rather than filling the page. -->
<div class="ui-section-group">
  <h4 id="usage-settings-title">{t('usage.title')}</h4>
  <div class="ui-setting-row">
    <div class="ui-setting-copy">
      <label for="usage-sharing">{t('usage.share')}</label>
      <p>{t('usage.shareHelp')}</p>
    </div>
    <label class="ui-setting-check"><input id="usage-sharing" type="checkbox" checked={preference?.enabled ?? false} disabled={!preference?.available || saving} onchange={async event => { const input=event.currentTarget; await change(input.checked); input.checked=preference?.enabled ?? false; }}><span>{t('privacy.enabled')}</span></label>
  </div>
  <details class="terms">
    <summary>{t('privacy.whatIsSent')}</summary>
    <p>{t('usage.terms1')}</p>
    <p>{t('usage.terms2')}<a href="https://github.com/tennoworth/tennoworth/blob/main/SECURITY.md" target="_blank" rel="noopener noreferrer">{t('usage.termsLink')}</a>{t('usage.terms3')}</p>
  </details>
  {#if !preference && !error}<p class="note" role="status">{t('privacy.loading')}</p>{/if}
  {#if preference && !preference.available}<p class="note">{t('privacy.unavailable')}</p>{/if}
  {#if error}<p class="note bad" role="alert">{error}</p>{/if}
</div>
<style>
  .terms { padding: var(--s2) var(--inset) 0; }
  .terms > summary { min-height: var(--ctl); cursor: pointer; color: var(--muted); font-size: var(--text-control); }
  .terms > p { margin: var(--s2) 0 0; max-width: 65ch; font-size: var(--text-control); line-height: var(--leading-body); color: var(--muted); }
  .note { margin: var(--s3) var(--inset) 0; font-size: var(--text-control); color: var(--muted); }
  .note.bad { color: var(--bad); }
</style>
