<script lang="ts">
  import { t } from '../../i18n/i18n.svelte';
  import type { DesktopCapabilities } from '../../contracts/desktop';
  import type { PriceReportPreferences } from '../../contracts/price-reports';
  import PromptBanner from '../prompts/PromptBanner.svelte';
  import type { PromptSession } from '../prompts/prompt-session.svelte';
  import { PRICE_SHARING_PROMPT } from '../prompts/policies';
  let { transport, session, active, onsettings }: {
    transport: DesktopCapabilities;
    session: PromptSession;
    /** False while the shell shows something that should not be crowded, and
     *  on Settings, where the full control is already in view. */
    active: boolean;
    onsettings: () => void;
  } = $props();
  let preference = $state<PriceReportPreferences | null>(null);
  let saving = $state(false);
  let error = $state('');
  let enabledHere = $state(false);
  // Re-read on each return from Settings, which may have changed the consent.
  $effect(() => {
    if (!active) return;
    void transport.getPriceReportPreferences().then(value => {
      preference = value;
      if (!value.available || value.enabled) session.pass(PRICE_SHARING_PROMPT.id);
    }).catch(() => { preference = null; session.pass(PRICE_SHARING_PROMPT.id); });
  });
  async function enable() {
    saving = true; error = '';
    try {
      preference = await transport.setPriceReportPreferences(true);
      if (preference.enabled) {
        enabledHere = true;
        session.finish(PRICE_SHARING_PROMPT.id);
      } else {
        error = t('prices.turnOnFailed');
      }
    } catch {
      error = t('prices.turnOnFailedRetry');
    } finally { saving = false; }
  }
</script>

{#if active && preference?.available && (!preference.enabled || enabledHere)}
  <PromptBanner {session} id={PRICE_SHARING_PROMPT.id} title={t(enabledHere ? 'prices.promptOn' : 'prices.promptAsk')} dismissLabel={t(enabledHere ? 'prices.close' : 'prices.notNow')}>
    {#if enabledHere}
      <p role="status">{t('prices.thanks')}</p>
    {:else}
      <p>{t('prices.pitch')}</p>
    {/if}
    {#if error}<p class="bad" role="alert">{error}</p>{/if}
    {#snippet actions()}
      {#if !enabledHere}
        <button type="button" class="btn primary" onclick={enable} disabled={saving}>{t(saving ? 'prices.turningOn' : 'prices.share')}</button>
      {/if}
      <button type="button" class="btn" onclick={onsettings}>{t(enabledHere ? 'prices.settings' : 'prices.whatIsSent')}</button>
    {/snippet}
  </PromptBanner>
{/if}

<style>
  .bad { color: var(--bad); }
</style>
