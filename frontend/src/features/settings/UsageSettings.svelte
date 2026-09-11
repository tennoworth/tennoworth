<script lang="ts">
  import { onMount } from 'svelte';
  import type { DesktopCapabilities } from '../../contracts/desktop';
  import type { UsagePreferences } from '../../contracts/usage';
  let { transport }: { transport: DesktopCapabilities } = $props();
  let preference = $state<UsagePreferences | null>(null);
  let saving = $state(false);
  let error = $state('');
  onMount(() => { void transport.getUsagePreferences().then(value => { preference=value; }).catch(() => { error='Could not load usage preference. No change has been made.'; }); });
  async function change(enabled: boolean) {
    saving=true; error='';
    try { preference=await transport.setUsagePreferences(enabled); }
    catch {
      try { preference=await transport.getUsagePreferences(); }
      catch { preference=null; }
      error='Could not save this preference. Retry before restarting the app.';
    }
    finally { saving=false; }
  }
</script>
<section class="wrap tw" aria-labelledby="usage-settings-title">
  <div class="rail"><h3 id="usage-settings-title">Usage sharing</h3></div>
  <div class="ui-setting-row">
    <div class="ui-setting-copy">
      <label for="usage-sharing">Share a daily usage count</label>
      <p>Off unless you choose to enable it. Contributes one installation per UTC day while the app is running, including in the tray, to our public community chart.</p>
      <p>At most one check-in attempt per UTC day, at startup or day rollover; no retries. Only a token that changes daily is sent. No account details, inventory, screenshots, hardware identifiers, or activity events. Turning this off stops future check-ins; previous aggregate counts remain.</p>
      <p>Delivery exposes your IP address to the hosting infrastructure. See the <a href="https://github.com/tennoworth/tennoworth/blob/main/SECURITY.md" target="_blank" rel="noopener noreferrer">security policy</a> for retention details.</p>
    </div>
    <label class="ui-setting-check"><input id="usage-sharing" type="checkbox" checked={preference?.enabled ?? false} disabled={!preference?.available || saving} onchange={async event => { const input=event.currentTarget; await change(input.checked); input.checked=preference?.enabled ?? false; }}><span>Enabled</span></label>
  </div>
  {#if !preference && !error}<p class="ui-panel-footer" role="status">Loading preference…</p>{/if}
  {#if preference && !preference.available}<p class="ui-panel-footer">Sharing is disabled in this build or launch.</p>{/if}
  {#if error}<p class="ui-panel-footer bad" role="alert">{error}</p>{/if}
</section>
