<script lang="ts">
  import { t } from '../../i18n/i18n.svelte';
  import Rich from '../../ui/Rich.svelte';
  import { onMount } from 'svelte';
  import type { DesktopCapabilities } from '../../contracts/desktop';
  import type { PriceReportPreferences } from '../../contracts/price-reports';
  let { transport }: { transport: DesktopCapabilities } = $props();
  let preference = $state<PriceReportPreferences | null>(null);
  let saving = $state(false);
  let erasing = $state(false);
  let erased = $state(false);
  let error = $state('');
  onMount(() => { void transport.getPriceReportPreferences().then(value => { preference=value; }).catch(() => { error=t('prices.loadFailed'); }); });
  async function change(enabled: boolean) {
    saving=true; error=''; erased=false;
    try { preference=await transport.setPriceReportPreferences(enabled); }
    catch {
      try { preference=await transport.getPriceReportPreferences(); }
      catch { preference=null; }
      error=t('privacy.saveFailed');
    }
    finally { saving=false; }
  }
  async function erase() {
    erasing=true; error=''; erased=false;
    try {
      await transport.erasePriceReports();
      erased=true;
      preference=await transport.getPriceReportPreferences();
    }
    catch { error=t('prices.eraseFailed'); }
    finally { erasing=false; }
  }
</script>
<!-- Beside usage sharing in Settings' "This app" panel: a set-once consent
     whose full terms sit in a disclosure. -->
<div class="ui-section-group" id="settings-price-sharing" tabindex="-1" aria-labelledby="price-settings-title">
  <h4 id="price-settings-title">{t('prices.title')}</h4>
  <div class="ui-setting-row">
    <div class="ui-setting-copy">
      <label for="price-sharing">{t('prices.share')}</label>
      <p>{t('prices.shareHelp')}</p>
      {#if preference?.enabled}<p role="status"><Rich text={t('prices.sharedThisWeek', { count: preference.sent_this_week })} /></p>{/if}
    </div>
    <label class="ui-setting-check"><input id="price-sharing" type="checkbox" checked={preference?.enabled ?? false} disabled={!preference?.available || saving} onchange={async event => { const input=event.currentTarget; await change(input.checked); input.checked=preference?.enabled ?? false; }}><span>{t('privacy.enabled')}</span></label>
  </div>
  <div class="ui-setting-row">
    <div class="ui-setting-copy">
      <strong>{t('prices.remove')}</strong>
      <p>{t('prices.removeHelp')}</p>
    </div>
    <div class="ui-setting-control"><button type="button" class="btn ghost" disabled={!preference?.available || erasing} onclick={erase}>{t(erasing ? 'prices.deleting' : 'prices.delete')}</button></div>
  </div>
  <details class="terms">
    <summary>{t('privacy.whatIsSent')}</summary>
    <p>{t('prices.terms1')}</p>
    <p>{t('prices.terms2')}</p>
    <p>{t('prices.terms3')}<a href="https://github.com/tennoworth/tennoworth/blob/main/docs/price-reports.md" target="_blank" rel="noopener noreferrer">{t('prices.termsLink')}</a>{t('prices.terms4')}</p>
  </details>
  {#if !preference && !error}<p class="note" role="status">{t('privacy.loading')}</p>{/if}
  {#if preference && !preference.available}<p class="note">{t('privacy.unavailable')}</p>{/if}
  {#if erased}<p class="note" role="status">{t('prices.erased')}</p>{/if}
  {#if error}<p class="note bad" role="alert">{error}</p>{/if}
</div>
<style>
  /* The shared count reads as a figure, as it did before translation. */
  [role="status"] :global(b) { font-family: var(--font-mono); font-weight: inherit; }
  #settings-price-sharing:focus { outline: none; }
  .terms { padding: var(--s2) var(--inset) 0; }
  .terms > summary { min-height: var(--ctl); cursor: pointer; color: var(--muted); font-size: var(--text-control); }
  .terms > p { margin: var(--s2) 0 0; max-width: 65ch; font-size: var(--text-control); line-height: var(--leading-body); color: var(--muted); }
  .note { margin: var(--s3) var(--inset) 0; font-size: var(--text-control); color: var(--muted); }
  .note.bad { color: var(--bad); }
</style>
