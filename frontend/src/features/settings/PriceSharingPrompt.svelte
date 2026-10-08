<script lang="ts">
  import type { DesktopCapabilities } from '../../contracts/desktop';
  import type { PriceReportPreferences } from '../../contracts/price-reports';
  import PromptBanner from '../../ui/PromptBanner.svelte';
  import type { PromptSession } from '../../ui/prompt-session.svelte';
  import { PRICE_SHARING_PROMPT } from '../prompt-policies';
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
        error = 'Price sharing could not be turned on. Nothing was changed.';
      }
    } catch {
      error = 'Price sharing could not be turned on. Nothing was changed; try again or use Settings.';
    } finally { saving = false; }
  }
</script>

{#if active && preference?.available && (!preference.enabled || enabledHere)}
  <PromptBanner {session} id={PRICE_SHARING_PROMPT.id} title={enabledHere ? 'Price sharing is on' : 'Help show what items really sell for'} dismissLabel={enabledHere ? 'Close' : 'Not now'}>
    {#if enabledHere}
      <p role="status">Thank you. Trades you complete from now on are shared. You can turn this off or delete recent reports in Settings at any time.</p>
    {:else}
      <p>Optional. When you trade for platinum, TennoWorth can share the item, quantity, price and day, and publishes an item’s price once five installs report it. It never sends who you traded with, your inventory or your accounts. Ignoring this changes nothing.</p>
    {/if}
    {#if error}<p class="bad" role="alert">{error}</p>{/if}
    {#snippet actions()}
      {#if !enabledHere}
        <button type="button" class="btn primary" onclick={enable} disabled={saving}>{saving ? 'Turning on…' : 'Share sale prices'}</button>
      {/if}
      <button type="button" class="btn" onclick={onsettings}>{enabledHere ? 'Settings' : 'What is sent'}</button>
    {/snippet}
  </PromptBanner>
{/if}

<style>
  .bad { color: var(--bad); }
</style>
