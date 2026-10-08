<script lang="ts">
  import type { DesktopCapabilities } from '../../contracts/desktop';
  import type { PriceReportPreferences } from '../../contracts/price-reports';
  import type { SettingsStore } from '../../contracts/state-store';
  import PromptBanner from '../../ui/PromptBanner.svelte';
  import { dismissPrompt } from '../../ui/prompts';
  const PROMPT_ID = 'price-sharing-v1';
  let { transport, store, active, onsettings }: {
    transport: DesktopCapabilities;
    store: SettingsStore;
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
    void transport.getPriceReportPreferences().then(value => { preference = value; }).catch(() => { preference = null; });
  });
  async function enable() {
    saving = true; error = '';
    try {
      preference = await transport.setPriceReportPreferences(true);
      if (preference.enabled) {
        enabledHere = true;
        void dismissPrompt(store, PROMPT_ID).catch(() => {});
      } else {
        error = 'Price sharing could not be turned on. Nothing was changed.';
      }
    } catch {
      error = 'Price sharing could not be turned on. Nothing was changed; try again or use Settings.';
    } finally { saving = false; }
  }
</script>

{#if active && preference?.available && (!preference.enabled || enabledHere)}
  <PromptBanner id={PROMPT_ID} {store} title={enabledHere ? 'Price sharing is on' : 'Help show what items really sell for'} dismissLabel={enabledHere ? 'Close' : 'Not now'}>
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
