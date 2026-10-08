<script lang="ts">
  import { useDesktopServices } from '../../ui/desktop-context';
  const { checkUpdate } = useDesktopServices();
  import UsageSettings from './UsageSettings.svelte';
  import PriceReportSettings from './PriceReportSettings.svelte';
  import NotificationSettings from './NotificationSettings.svelte';
  import ThemeSwitcher from '../../ui/ThemeSwitcher.svelte';
  import AppIconSetting from './AppIconSetting.svelte';
  import type { AppIconController } from './app-icon.svelte';
  import { onMount, tick } from 'svelte';
  import type { ThemeController } from '../../ui/theme';
  import type { DesktopWfmStatus, DesktopCapabilities } from '../../contracts/desktop';
  import { AUTO_SCAN_CADENCE_CHOICES, type AutoScanSettings, type AutoScanStatus } from '../../contracts/desktop';
  import type { AutoScanController } from '../inventory/auto-scan.svelte';
  import type { OverlaySettings, OverlayStatus } from '../../contracts/data';
  
import { type UpdateStatus } from '../../contracts/update';
  import { humanError } from '../../contracts/errors';

  interface Props {
    /** The boot-time controller from src/lib/theme.ts. */
    theme: ThemeController;
    onwhatsnew?: () => void;
    transport?: DesktopCapabilities;
    /** Desktop only. The shell owns the instance, so the panel and the shell
     *  read one source of truth for these preferences and this status. */
    autoScan?: AutoScanController;
    /** Desktop only: the window and tray icon colour. */
    appIcon?: AppIconController;
    wfmStatus?: DesktopWfmStatus | null;
    onwfmlogout?: () => Promise<void>;
    /** Opens the page at this section (the inbox's "Notification settings",
     *  the price sharing prompt's "What is sent"). */
    section?: 'notifications' | 'price-sharing' | null;
    onsectionshown?: () => void;
  }
  let { theme, onwhatsnew, transport, autoScan, appIcon, wfmStatus = null, onwfmlogout, section = null, onsectionshown }: Props = $props();

  let overlay = $state<OverlaySettings | null>(null);
  let overlayStatus = $state<OverlayStatus | null>(null);
  let overlayError = $state('');
  let savingOverlay = $state(false);
  let checkingUpdate = $state(false);
  let checkedUpdate = $state<UpdateStatus | null>(null);
  let updateError = $state('');
  let confirmingLogout = $state(false);
  let loggingOut = $state(false);
  let logoutError = $state('');
  let savingAutoScan = $state(false);
  let statusGeneration = 0;
  let statusPolling = false;
  let disposed = false;
  // Diagnostics are for when recognition breaks, so they start collapsed -
  // unless they are already on, when their warning must stay in view.
  let troubleshootingOpen = $state(false);

  $effect(() => {
    void wfmStatus?.logged_in;
    void wfmStatus?.unlocked;
    confirmingLogout = false;
    logoutError = '';
  });

  // Waits for the overlay settings: they load after mount and would push a
  // section scrolled to earlier down out of view.
  async function showSection(loaded: Promise<unknown>) {
    if (!section) return;
    await loaded;
    if (disposed) return;
    await tick();
    const target = document.getElementById(`settings-${section}`);
    target?.scrollIntoView({ block: 'start' });
    target?.focus({ preventScroll: true });
    onsectionshown?.();
  }

  async function refreshOverlayStatus(read?: () => Promise<OverlayStatus>): Promise<void> {
    if (!transport || disposed) return;
    const generation = ++statusGeneration;
    const next = await (read ? read() : transport.overlayStatus());
    if (!disposed && generation === statusGeneration) overlayStatus = next;
  }

  onMount(() => {
    if (!transport) { void showSection(Promise.resolve()); return; }
    const initial = () => Promise.all([transport.getOverlaySettings(), refreshOverlayStatus()])
      .then(([settings]) => { if (!disposed) { overlay = settings; troubleshootingOpen = settings.diagnostics; } })
      .catch((error) => { if (!disposed) overlayError = String(error); });
    const refreshStatus = async () => {
      if (!overlay || statusPolling || savingOverlay || disposed) return;
      statusPolling = true;
      try { await refreshOverlayStatus(); }
      catch { /* The next poll retries while the last status remains visible. */ }
      finally { statusPolling = false; }
    };
    void showSection(initial());
    const timer = window.setInterval(() => {
      void refreshStatus();
      // The loop's own view: whether the game is up, when the next scan is due,
      // and why the last one failed.
      void autoScan?.refreshStatus();
    }, 1000);
    return () => { disposed = true; window.clearInterval(timer); };
  });

  /** What the background loop is doing right now, in the user's terms. */
  function autoScanStatusText(status: AutoScanStatus | null): string {
    if (!status) return 'Reading status…';
    if (!status.enabled) return 'Manual scans only.';
    if (status.held) return 'Automatic scan paused while a listing review or Trade Session is open.';
    if (status.lastError) return `Last automatic scan failed: ${status.lastError}`;
    if (!status.gameRunning) return 'Waiting for Warframe. Nothing is scanned while the game is closed.';
    const last = status.lastScanAt ? `Last automatic scan ${new Date(status.lastScanAt * 1000).toLocaleTimeString()}.` : '';
    const next = status.nextCheckAt ? `Next scan around ${new Date(status.nextCheckAt * 1000).toLocaleTimeString()}.` : '';
    return [last, next].filter(Boolean).join(' ') || 'Watching for Warframe.';
  }

  function autoScanSummary(): string {
    const settings = autoScan?.settings;
    if (!settings) return 'Loading…';
    return settings.enabled ? `On · every ${settings.cadenceMinutes} min` : 'Off';
  }

  function overlaySummary(): string {
    if (!overlay) return 'Loading…';
    if (!overlay.enabled) return 'Off';
    if (!overlayStatus) return 'On';
    return `${overlayStatus.state.replaceAll('-', ' ')} · ${overlayStatus.ocrReady ? 'OCR ready' : 'OCR unavailable'}`;
  }

  function lastRunText(): string {
    const run = overlayStatus?.lastRun;
    return run ? `Last run: ${run.outcome} · ${run.recognizedSlots}/${run.expectedSlots || '?'} slots · ${run.timings.totalMs} ms` : 'No reward screen read yet.';
  }

  function sessionText(): string {
    if (!wfmStatus) return 'Checking session…';
    if (wfmStatus.unlocked) return 'Signed in · session unlocked';
    if (wfmStatus.logged_in) return 'Signed in · session locked';
    return 'Not signed in';
  }

  // `revert` puts the control back to the stored value. A failed save leaves the
  // settings unchanged, so nothing re-renders and the control would keep showing
  // a value that was never stored - and the next click would send its opposite.
  async function saveAutoScan(next: AutoScanSettings, revert: (stored: AutoScanSettings) => void) {
    if (!autoScan) return;
    savingAutoScan = true;
    try {
      await autoScan.save(next);
    } finally {
      savingAutoScan = false;
    }
    if (autoScan.settingsError && autoScan.settings) revert(autoScan.settings);
  }

  async function saveOverlay(next: OverlaySettings) {
    if (!transport) return;
    const wasEnabled = overlay?.enabled ?? false;
    overlayError = '';
    savingOverlay = true;
    statusGeneration++;
    try {
      overlay = await transport.updateOverlaySettings(next);
      await refreshOverlayStatus(!wasEnabled && overlay.enabled ? () => transport!.setupOverlayCapture() : undefined);
    } catch (error) {
      overlayError = error instanceof Error ? error.message : String(error);
    } finally {
      savingOverlay = false;
    }
  }

  async function testOverlay() {
    if (!transport) return;
    overlayError = '';
    try {
      await transport.scanOverlayNow();
      await refreshOverlayStatus();
    } catch (error) {
      overlayError = error instanceof Error ? error.message : String(error);
    }
  }

  async function previewOverlay() {
    if (!transport) return;
    overlayError = '';
    try {
      await transport.previewRelicOverlay();
      await refreshOverlayStatus();
    } catch (error) {
      overlayError = error instanceof Error ? error.message : String(error);
    }
  }

  async function diagnosticsAction(action: 'open' | 'clear') {
    if (!transport) return;
    overlayError = '';
    try {
      if (action === 'open') await transport.openOverlayDiagnostics();
      else await transport.clearOverlayDiagnostics();
      await refreshOverlayStatus();
    } catch (error) {
      overlayError = error instanceof Error ? error.message : String(error);
    }
  }

  async function checkForUpdates() {
    updateError = '';
    checkingUpdate = true;
    try {
      checkedUpdate = await checkUpdate();
    } catch (error) {
      updateError = humanError(error);
    } finally {
      checkingUpdate = false;
    }
  }

  async function logOutWfm() {
    if (!onwfmlogout || loggingOut) return;
    if (!confirmingLogout) {
      confirmingLogout = true;
      return;
    }
    logoutError = '';
    loggingOut = true;
    try {
      await onwfmlogout();
      confirmingLogout = false;
    } catch (error) {
      logoutError = humanError(error);
    } finally {
      loggingOut = false;
    }
  }
</script>

<section class="view-header">
  <h2>Settings</h2>
  <p class="lede">Preferences for this device.</p>
</section>

<div class="settings ui-reading">
  <!-- Most visits ask "is it running?", so the answers lead and link to the
       controls that change them. -->
  <div class="ui-summary-strip glance" aria-label="Current state">
    {#if autoScan}<a href="#settings-scan"><span class="k">Automatic scan</span><strong>{autoScanSummary()}</strong><span>{autoScanStatusText(autoScan.status)}</span></a>{/if}
    {#if transport}<a href="#settings-overlay"><span class="k">Relic overlay</span><strong>{overlaySummary()}</strong><span>{overlay?.enabled ? lastRunText() : 'Reward screens are not read.'}</span></a>{/if}
    <a href="#settings-account"><span class="k">warframe.market</span><strong class:ok={wfmStatus?.unlocked}>{sessionText()}</strong><span>Encrypted login on this device</span></a>
  </div>
  <nav class="jump" aria-label="Settings sections">
    <span class="k">Everyday</span>
    {#if autoScan}<a href="#settings-scan">Automatic scan</a>{/if}
    <a href="#settings-overlay">Relic overlay</a>
    <a href="#settings-notifications">Notifications</a>
    <span class="k">Set once</span>
    <a href="#settings-account">Account</a>
    <a href="#settings-app">Appearance, updates and privacy</a>
  </nav>

  <p class="tier">Everyday</p>
  {#if autoScan}
    <section class="wrap tw" id="settings-scan" tabindex="-1" aria-labelledby="set-auto-scan">
      <div class="rail"><h3 id="set-auto-scan">Automatic scan</h3></div>
      {#if autoScan.settings}
        <div class="ui-setting-row">
          <div class="ui-setting-copy"><strong>Status</strong><p>{autoScanStatusText(autoScan.status)}</p></div>
        </div>
        <div class="ui-setting-row">
          <div class="ui-setting-copy"><label for="auto-scan-enabled">Scan automatically while Warframe is running</label><p>Off: TennoWorth scans only when you ask. On: it looks for the game and rescans on the chosen cadence. Each rescan reads the game's memory and requests your account inventory from Digital Extremes. Nothing is scanned while the game is closed or logged out, and a rescan never interrupts a listing review.</p></div>
          <div class="ui-setting-control">
            <label class="ui-setting-check"><input id="auto-scan-enabled" type="checkbox" checked={autoScan.settings.enabled} disabled={savingAutoScan} onchange={(event) => { const input = event.currentTarget; void saveAutoScan({ ...autoScan!.settings!, enabled: input.checked }, (stored) => (input.checked = stored.enabled)); }}><span>Enabled</span></label>
            {#if autoScan.settings.enabled}
              <select id="auto-scan-cadence" class="ui-input" aria-label="Scan every" value={autoScan.settings.cadenceMinutes} disabled={savingAutoScan} onchange={(event) => { const select = event.currentTarget; void saveAutoScan({ ...autoScan!.settings!, cadenceMinutes: Number(select.value) }, (stored) => (select.value = String(stored.cadenceMinutes))); }}>{#each AUTO_SCAN_CADENCE_CHOICES as minutes}<option value={minutes}>every {minutes} minutes</option>{/each}</select>
            {/if}
          </div>
        </div>
        <div class="ui-setting-row">
          <div class="ui-setting-copy"><label for="auto-scan-adopt">Update the open app automatically</label><p>When a scan finishes in the background - on the cadence, or Rescan from the tray - replace the inventory on screen. While a listing review or Trade Session is being prepared the scan waits for “Load new scan” instead.</p></div>
          <label class="ui-setting-check"><input id="auto-scan-adopt" type="checkbox" checked={autoScan.settings.adoptAutomatically} disabled={savingAutoScan} onchange={(event) => { const input = event.currentTarget; void saveAutoScan({ ...autoScan!.settings!, adoptAutomatically: input.checked }, (stored) => (input.checked = stored.adoptAutomatically)); }}><span>Automatic</span></label>
        </div>
      {:else}<p class="exp inset">Loading automatic-scan settings…</p>{/if}
      {#if autoScan.settingsError}<p class="error inset" role="alert">{autoScan.settingsError}</p>{/if}
    </section>
  {/if}
  <section class="wrap tw" id="settings-overlay" tabindex="-1" aria-labelledby="set-relic-overlay">
    <div class="rail">
      <h3 id="set-relic-overlay">Relic reward overlay</h3>
      {#if overlay}<button class="btn xs" onclick={previewOverlay} disabled={!overlay.enabled || savingOverlay}>Preview overlay</button><button class="btn xs" onclick={testOverlay} disabled={!overlay.enabled || savingOverlay}>Scan reward screen now</button>{/if}
    </div>
    {#if overlay}
      <div class="ui-setting-row">
        <div class="ui-setting-copy"><label for="overlay-enabled">Enable local screen recognition</label><p>Captures only after a reward event or your retry shortcut. Frames stay in memory and are never uploaded.</p>{#if overlay.enabled && overlayStatus?.lastRun}<p>{lastRunText()}</p>{/if}</div>
        <div class="ui-setting-control">
          <label class="ui-setting-check"><input id="overlay-enabled" type="checkbox" checked={overlay.enabled} disabled={savingOverlay} onchange={(event) => saveOverlay({ ...overlay!, enabled: event.currentTarget.checked })}><span>Enabled</span></label>
          {#if overlayStatus}<span class="status" class:good={['watching', 'showing'].includes(overlayStatus.state)}>{overlayStatus.state.replaceAll('-', ' ')} · {overlayStatus.ocrReady ? 'OCR ready' : 'OCR unavailable'}</span>{/if}
        </div>
      </div>
      <div class="split">
        <div class="ui-section-group">
          <h4>Detection</h4>
          <div class="ui-setting-row">
            <div class="ui-setting-copy"><label for="overlay-auto">Automatic reward detection</label><p>Watches EE.log for “Got rewards”.</p></div>
            <label class="ui-setting-check"><input id="overlay-auto" type="checkbox" checked={overlay.autoDetect} disabled={!overlay.enabled || savingOverlay} onchange={(event) => saveOverlay({ ...overlay!, autoDetect: event.currentTarget.checked })}><span>Enabled</span></label>
          </div>
          <div class="ui-setting-row"><div class="ui-setting-copy"><label for="overlay-shortcut">Retry shortcut</label><p>Still available when the game delays that line.</p></div><div class="ui-setting-control"><input id="overlay-shortcut" class="ui-input shortcut" value={overlay.shortcut} disabled={!overlay.enabled || savingOverlay} onblur={(event) => saveOverlay({ ...overlay!, shortcut: event.currentTarget.value })}></div></div>
        </div>
        <div class="ui-section-group">
          <h4>Reward cards</h4>
          <div class="ui-setting-row"><div class="ui-setting-copy"><label for="overlay-scale">Card scale</label></div><div class="ui-setting-control"><input id="overlay-scale" type="range" min="0.75" max="1.5" step="0.05" value={overlay.scale} disabled={!overlay.enabled || savingOverlay} onchange={(event) => saveOverlay({ ...overlay!, scale: Number(event.currentTarget.value) })}><output class="mono" for="overlay-scale">{Math.round(overlay.scale * 100)}%</output></div></div>
          <div class="ui-setting-row"><div class="ui-setting-copy"><label for="overlay-prices">Live online asks</label><p>Replace cached prices with live online asks.</p></div><label class="ui-setting-check"><input id="overlay-prices" type="checkbox" checked={overlay.livePrices} disabled={!overlay.enabled || savingOverlay} onchange={(event) => saveOverlay({ ...overlay!, livePrices: event.currentTarget.checked })}><span>Enabled</span></label></div>
          <div class="ui-setting-row"><div class="ui-setting-copy"><label for="overlay-owned">Owned count</label><p>Show count from the latest inventory scan.</p></div><label class="ui-setting-check"><input id="overlay-owned" type="checkbox" checked={overlay.showOwned} disabled={!overlay.enabled || savingOverlay} onchange={(event) => saveOverlay({ ...overlay!, showOwned: event.currentTarget.checked })}><span>Shown</span></label></div>
        </div>
      </div>
      <details class="troubleshooting" bind:open={troubleshootingOpen}>
        <summary>Troubleshooting: diagnostics and display requirements</summary>
        <div class="ui-setting-row"><div class="ui-setting-copy"><label for="overlay-diagnostics">Save local recognition diagnostics</label><p>Keep captures on this device to investigate recognition problems.</p></div><label class="ui-setting-check"><input id="overlay-diagnostics" type="checkbox" checked={overlay.diagnostics} disabled={!overlay.enabled || savingOverlay} onchange={(event) => saveOverlay({ ...overlay!, diagnostics: event.currentTarget.checked })}><span>Enabled</span></label></div>
        {#if overlay.diagnostics}<div class="diagnostics"><p class="warning">Diagnostic captures may contain player or game information. They stay on this device and are never uploaded automatically.</p><div class="ui-toolbar"><button class="btn" onclick={() => diagnosticsAction('open')}>Open diagnostics</button><button class="btn" onclick={() => diagnosticsAction('clear')}>Clear diagnostics</button></div></div>{/if}
        <div class="requirements">
          {#if overlayStatus}<p class="status">{overlayStatus.backend} capture · {overlayStatus.presentationBackend} display</p>{/if}
          <p class="exp">Use Borderless Fullscreen or Windowed mode. Windows and X11 use direct window capture; Wayland captures through XWayland and presents through layer-shell when the compositor supports it. No interaction, injection, or automatic reward selection is performed.</p>
        </div>
      </details>
    {:else}<p class="exp inset">Loading overlay settings…</p>{/if}
    {#if overlayError}<p class="error inset" role="alert">{overlayError}</p>{/if}
  </section>
  <NotificationSettings />

  <p class="tier">Set once</p>
  <section class="wrap tw" id="settings-account" tabindex="-1" aria-labelledby="set-wfm-account">
    <div class="rail"><h3 id="set-wfm-account">warframe.market account</h3></div>
    <div class="ui-setting-row">
      <div class="ui-setting-copy"><strong>Account session</strong><p>{#if wfmStatus?.logged_in || wfmStatus?.unlocked}Logging out removes the encrypted login saved on this device, forgets its remembered unlock key, and discards any interrupted local listing batch. Your listings on warframe.market are not changed.{:else}Manage the encrypted login saved on this device.{/if}</p></div>
      <div class="ui-setting-control">
        <span class="status" class:ok={wfmStatus?.unlocked}>{sessionText()}</span>
        {#if wfmStatus?.logged_in || wfmStatus?.unlocked}
          <button class="btn" class:bad={confirmingLogout} onclick={logOutWfm} disabled={loggingOut}>{loggingOut ? 'Logging out…' : confirmingLogout ? 'Confirm log out' : 'Log out'}</button>
          {#if confirmingLogout}<button class="btn ghost" onclick={() => { confirmingLogout = false; logoutError = ''; }} disabled={loggingOut}>Cancel</button>{/if}
        {/if}
      </div>
    </div>
    {#if logoutError}<p class="error inset" role="alert">Couldn’t log out: {logoutError}</p>{/if}
  </section>
  <!-- Each of these is one row, set once. As three panels they used to lead the page. -->
  <section class="wrap tw" id="settings-app" tabindex="-1" aria-labelledby="set-app">
    <div class="rail"><h3 id="set-app">This app</h3></div>
    <div class="ui-section-group first">
      <h4>Appearance</h4>
      <div class="ui-setting-row">
        <div class="ui-setting-copy"><strong>Colour mode</strong><p>System follows your operating system's light/dark setting and changes with it; Light and Dark pin the app regardless.</p></div>
        <div class="ui-setting-control"><ThemeSwitcher {theme} label="Colour mode" /></div>
      </div>
      {#if appIcon}<AppIconSetting {appIcon} />{/if}
    </div>
    <div class="ui-section-group">
      <h4>Updates</h4>
      <div class="ui-setting-row">
        <div class="ui-setting-copy"><strong>Application updates</strong><p>On Windows and Linux AppImage, TennoWorth also checks every 30 minutes while it is running. Updates are downloaded and installed only after you confirm.</p></div>
        <div class="ui-setting-control">
          {#if onwhatsnew}<button class="btn" onclick={onwhatsnew}>What’s new</button>{/if}
          <button class="btn" onclick={checkForUpdates} disabled={checkingUpdate}>{checkingUpdate ? 'Checking…' : 'Check for updates'}</button>
          {#if checkedUpdate?.available}<span class="status">Version {checkedUpdate.version} is available.</span>
          {:else if checkedUpdate?.checked && checkedUpdate.support === 'supported'}<span class="status">You’re up to date · v{checkedUpdate.current_version}</span>
          {:else if checkedUpdate?.checked && checkedUpdate.support === 'appimage_required'}<span class="status">This install can’t update itself. Download and run the TennoWorth AppImage to receive updates.</span>
          {:else if checkedUpdate?.checked && checkedUpdate.support === 'disabled_test_build'}<span class="status">Updates are disabled in this test build.</span>{/if}
        </div>
      </div>
      {#if updateError}<p class="error inset" role="alert">{updateError}</p>{/if}
    </div>
    {#if transport}<UsageSettings {transport} /><PriceReportSettings {transport} />{/if}
  </section>
</div>

<style>
  .settings { display: flex; flex-direction: column; gap: var(--s4); }
  .lede { margin: 0; color: var(--muted); }
  .inset { padding: 0 var(--inset) var(--s4); }
  .exp { margin: 0; font-size: var(--text-control); line-height: var(--leading-body); color: var(--muted); max-width: 85ch; }
  .shortcut { width: 100%; font-family: var(--font-mono); }
  input[type='range'] { width: 8rem; min-height: var(--ctl-lg); accent-color: var(--accent); }
  .mono { font-family: var(--font-mono); font-size: var(--text-caption); color: var(--muted); }
  /* Status words are prose, not data: body face, with a square for "signed in". */
  .status { font-size: var(--text-control); color: var(--muted); overflow-wrap: anywhere; }
  .status.ok::before, .glance strong.ok::before { content: ""; display: inline-block; width: 7px; height: 7px; margin-right: var(--s2); background: var(--good); vertical-align: 1px; }
  .good { color: var(--good); }
  .diagnostics { padding: 0 var(--inset) var(--s4); display: flex; flex-direction: column; gap: var(--s3); }
  .warning { margin: 0; padding-left: var(--s3); border-left: 2px solid var(--warn); color: var(--warn); font-size: var(--text-control); line-height: var(--leading-body); }
  .error { margin: 0; color: var(--bad); font-size: var(--text-control); }

  /* The strip holds one to three cells, depending on what this build offers. */
  .glance { grid-template-columns: repeat(auto-fit, minmax(min(12rem, 100%), 1fr)); }
  .glance > a { display: flex; flex-direction: column; gap: var(--s1); padding: var(--s3) var(--inset); min-width: 0; color: var(--fg); border-bottom: 0; }
  .glance > a + a { border-left: 1px var(--rule) var(--border); }
  .glance > a:hover { background: var(--panel-2); }
  .glance strong { font: 500 var(--text-body)/var(--leading-control) var(--font-body); overflow-wrap: anywhere; }
  .glance > a > span:last-child { font-size: var(--text-caption); line-height: var(--leading-body); color: var(--muted); overflow-wrap: anywhere; }
  .jump { display: flex; flex-wrap: wrap; align-items: center; gap: var(--s2) var(--s4); font-size: var(--text-control); }
  .jump .k, .tier { font: 600 var(--text-caption)/var(--leading-control) var(--font-ui); letter-spacing: .12em; text-transform: uppercase; color: var(--muted); }
  .jump .k:not(:first-child) { margin-left: var(--s2); padding-left: var(--s4); border-left: 1px var(--rule) var(--border); }
  .tier { display: flex; align-items: center; gap: var(--s3); margin: var(--s2) 0 calc(var(--s2) * -1); }
  .tier::after { content: ""; flex: 1; border-top: 1px var(--rule) var(--border); }
  section[tabindex="-1"]:focus { outline: none; }

  /* Detection and Reward cards side by side while each column can still hold
     a label beside its control; they stack with the rows below 760px. */
  .split { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); border-top: 1px var(--rule) var(--border); }
  .split > .ui-section-group { border-top: 0; min-width: 0; }
  .split > .ui-section-group + .ui-section-group { border-left: 1px var(--rule) var(--border); }
  .split .ui-setting-row { grid-template-columns: minmax(0, 1fr) auto; gap: var(--s3); }
  .split .shortcut { width: 9rem; }
  .troubleshooting { border-top: 1px var(--rule) var(--border); }
  .troubleshooting > summary { min-height: var(--ctl-lg); padding: var(--s1) var(--inset); cursor: pointer; color: var(--muted); font-size: var(--text-control); }
  .troubleshooting > .ui-setting-row { padding-top: var(--s2); }
  .requirements { display: flex; flex-direction: column; gap: var(--s2); padding: 0 var(--inset) var(--s4); }
  .requirements p { margin: 0; }
  .ui-section-group.first { border-top: 0; }

  @media (max-width: 1050px) { .split { grid-template-columns: minmax(0, 1fr); } .split > .ui-section-group + .ui-section-group { border-left: 0; border-top: 1px var(--rule) var(--border); } }
  @media (max-width: 760px) { .split .ui-setting-row { grid-template-columns: minmax(0, 1fr); gap: var(--s2); } .jump .k:not(:first-child) { margin-left: 0; padding-left: 0; border-left: 0; } }
</style>
