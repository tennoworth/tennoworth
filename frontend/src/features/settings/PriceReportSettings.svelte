<script lang="ts">
  import { onMount } from 'svelte';
  import type { DesktopCapabilities } from '../../contracts/desktop';
  import type { PriceReportPreferences } from '../../contracts/price-reports';
  let { transport }: { transport: DesktopCapabilities } = $props();
  let preference = $state<PriceReportPreferences | null>(null);
  let saving = $state(false);
  let erasing = $state(false);
  let erased = $state(false);
  let error = $state('');
  onMount(() => { void transport.getPriceReportPreferences().then(value => { preference=value; }).catch(() => { error='Could not load the price sharing preference. No change has been made.'; }); });
  async function change(enabled: boolean) {
    saving=true; error=''; erased=false;
    try { preference=await transport.setPriceReportPreferences(enabled); }
    catch {
      try { preference=await transport.getPriceReportPreferences(); }
      catch { preference=null; }
      error='Could not save this preference. Retry before restarting the app.';
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
    catch { error='Could not delete this week’s reports. Nothing was removed; try again later.'; }
    finally { erasing=false; }
  }
</script>
<!-- Beside usage sharing in Settings' "This app" panel: a set-once consent
     whose full terms sit in a disclosure. -->
<div class="ui-section-group">
  <h4 id="price-settings-title">Price sharing</h4>
  <div class="ui-setting-row">
    <div class="ui-setting-copy">
      <label for="price-sharing">Share sale prices</label>
      <p>Off unless you choose to enable it. When a trade for platinum completes from now on, the item and the price are sent so TennoWorth can publish what items really sell for. An item’s price is published only once five installs report it in the same week.</p>
      {#if preference?.enabled}<p role="status"><span class="mono">{preference.sent_this_week}</span> {preference.sent_this_week === 1 ? 'sale' : 'sales'} shared this week.</p>{/if}
    </div>
    <label class="ui-setting-check"><input id="price-sharing" type="checkbox" checked={preference?.enabled ?? false} disabled={!preference?.available || saving} onchange={async event => { const input=event.currentTarget; await change(input.checked); input.checked=preference?.enabled ?? false; }}><span>Enabled</span></label>
  </div>
  <div class="ui-setting-row">
    <div class="ui-setting-copy">
      <strong>Remove what you shared</strong>
      <p>Deletes this install’s reports from this week and last week. Older weeks are already merged into totals that no longer link to any install.</p>
    </div>
    <div class="ui-setting-control"><button type="button" class="btn ghost" disabled={!preference?.available || erasing} onclick={erase}>{erasing ? 'Deleting…' : 'Delete recent reports'}</button></div>
  </div>
  <details class="terms">
    <summary>What is sent and what is not</summary>
    <p>Only trades where platinum was exchanged for one kind of item: the item’s market name, quantity, price, whether it was a sale or a purchase, and the UTC day. Trades from before you enabled this are never sent. A trade that could not be sent is retried, with growing pauses, for up to seven days.</p>
    <p>Never sent: who you traded with, the time of day, your inventory, riven trades, your game or warframe.market account, the app version or your platform. Reports carry an id that changes every week, so weeks cannot be linked to each other or to you.</p>
    <p>Mod and arcane ranks are not in the game’s trade log, so those prices are published as “rank unknown”. Delivery exposes your IP address to the hosting infrastructure. See <a href="https://github.com/tennoworth/tennoworth/blob/main/docs/price-reports.md" target="_blank" rel="noopener noreferrer">how price reports work</a> for retention and publishing details.</p>
  </details>
  {#if !preference && !error}<p class="note" role="status">Loading preference…</p>{/if}
  {#if preference && !preference.available}<p class="note">Sharing is disabled in this build or launch.</p>{/if}
  {#if erased}<p class="note" role="status">Recent reports deleted from the service.</p>{/if}
  {#if error}<p class="note bad" role="alert">{error}</p>{/if}
</div>
<style>
  .terms { padding: var(--s2) var(--inset) 0; }
  .terms > summary { min-height: var(--ctl); cursor: pointer; color: var(--muted); font-size: var(--text-control); }
  .terms > p { margin: var(--s2) 0 0; max-width: 65ch; font-size: var(--text-control); line-height: var(--leading-body); color: var(--muted); }
  .note { margin: var(--s3) var(--inset) 0; font-size: var(--text-control); color: var(--muted); }
  .note.bad { color: var(--bad); }
</style>
