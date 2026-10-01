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
<!-- A group inside Settings' "This app" panel: set once, so the full privacy
     terms sit one click away rather than filling the page. -->
<div class="ui-section-group">
  <h4 id="usage-settings-title">Usage sharing</h4>
  <div class="ui-setting-row">
    <div class="ui-setting-copy">
      <label for="usage-sharing">Share a daily usage count</label>
      <p>Off unless you choose to enable it. Contributes one installation per UTC day while the app is running, including in the tray, to our public community chart.</p>
    </div>
    <label class="ui-setting-check"><input id="usage-sharing" type="checkbox" checked={preference?.enabled ?? false} disabled={!preference?.available || saving} onchange={async event => { const input=event.currentTarget; await change(input.checked); input.checked=preference?.enabled ?? false; }}><span>Enabled</span></label>
  </div>
  <details class="terms">
    <summary>What is sent and what is not</summary>
    <p>One check-in per UTC day, sent at startup or day rollover. If it does not get through, the same token is offered again once a minute, at most 10 times that day, and never for a past day. Only a token that changes daily is sent. No account details, inventory, screenshots, hardware identifiers, or activity events. Turning this off stops future check-ins; previous aggregate counts remain.</p>
    <p>Delivery exposes your IP address to the hosting infrastructure. See the <a href="https://github.com/tennoworth/tennoworth/blob/main/SECURITY.md" target="_blank" rel="noopener noreferrer">security policy</a> for retention details.</p>
  </details>
  {#if !preference && !error}<p class="note" role="status">Loading preference…</p>{/if}
  {#if preference && !preference.available}<p class="note">Sharing is disabled in this build or launch.</p>{/if}
  {#if error}<p class="note bad" role="alert">{error}</p>{/if}
</div>
<style>
  .terms { padding: var(--s2) var(--inset) 0; }
  .terms > summary { min-height: var(--ctl); cursor: pointer; color: var(--muted); font-size: var(--text-control); }
  .terms > p { margin: var(--s2) 0 0; max-width: 65ch; font-size: var(--text-control); line-height: var(--leading-body); color: var(--muted); }
  .note { margin: var(--s3) var(--inset) 0; font-size: var(--text-control); color: var(--muted); }
  .note.bad { color: var(--bad); }
</style>
