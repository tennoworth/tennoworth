<script lang="ts">
  import { useDesktopServices } from '../../ui/desktop-context';
  const { checkUpdate } = useDesktopServices();
  import UsageSettings from './UsageSettings.svelte';
  import NotificationSettings from './NotificationSettings.svelte';
  import ThemeSwitcher from '../../ui/ThemeSwitcher.svelte';
  import { onMount } from 'svelte';
  import type { ThemeController } from '../../ui/theme';
  import type { DesktopWfmStatus, DesktopCapabilities } from '../../contracts/desktop';
  import type { OverlaySettings, OverlayStatus } from '../../contracts/data';
  
import { type UpdateStatus } from '../../contracts/update';
  import { humanError } from '../../contracts/errors';

  interface Props {
    /** The boot-time controller from src/lib/theme.ts. */
    theme: ThemeController;
    transport?: DesktopCapabilities;
    isDesktop?: boolean;
    wfmStatus?: DesktopWfmStatus | null;
    onwfmlogout?: () => Promise<void>;
  }
  let { theme, transport, isDesktop = false, wfmStatus = null, onwfmlogout }: Props = $props();

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

  $effect(() => {
    void wfmStatus?.logged_in;
    void wfmStatus?.unlocked;
    confirmingLogout = false;
    logoutError = '';
  });

  onMount(() => {
    if (!isDesktop || !transport) return;
    const initial = () => Promise.all([transport.getOverlaySettings(), transport.overlayStatus()])
      .then(([settings, status]) => { overlay = settings; overlayStatus = status; })
      .catch((error) => { overlayError = String(error); });
    const refreshStatus = () => transport.overlayStatus()
      .then((status) => { overlayStatus = status; })
      .catch(() => {});
    void initial();
    const timer = window.setInterval(() => { void refreshStatus(); }, 1000);
    return () => window.clearInterval(timer);
  });

  async function saveOverlay(next: OverlaySettings) {
    if (!transport) return;
    const wasEnabled = overlay?.enabled ?? false;
    overlayError = '';
    savingOverlay = true;
    try {
      overlay = await transport.updateOverlaySettings(next);
      overlayStatus = !wasEnabled && overlay.enabled
        ? await transport.setupOverlayCapture()
        : await transport.overlayStatus();
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
      overlayStatus = await transport.overlayStatus();
    } catch (error) {
      overlayError = error instanceof Error ? error.message : String(error);
    }
  }

  async function previewOverlay() {
    if (!transport) return;
    overlayError = '';
    try {
      await transport.previewRelicOverlay();
      overlayStatus = await transport.overlayStatus();
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
      overlayStatus = await transport.overlayStatus();
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
  <section class="wrap tw" aria-labelledby="set-appearance">
    <div class="rail"><h3 id="set-appearance">Appearance</h3></div>
    <div class="ui-setting-row">
      <div class="ui-setting-copy"><strong>Colour mode</strong><p>System follows your operating system's light/dark setting and changes with it; Light and Dark pin the app regardless.</p></div>
      <div class="ui-setting-control"><ThemeSwitcher {theme} label="Colour mode" /></div>
    </div>
  </section>
  {#if isDesktop}
    <section class="wrap tw" aria-labelledby="set-wfm-account">
      <div class="rail"><h3 id="set-wfm-account">warframe.market account</h3></div>
      <div class="ui-setting-row">
        <div class="ui-setting-copy"><strong>Account session</strong><p>Manage the encrypted login saved on this device.</p></div>
        <div class="ui-setting-control">
          <span class="status">{#if !wfmStatus}Checking session…{:else if wfmStatus.unlocked}Signed in · session unlocked{:else if wfmStatus.logged_in}Signed in · session locked{:else}Not signed in{/if}</span>
          {#if wfmStatus?.logged_in || wfmStatus?.unlocked}
            <button class="btn" class:bad={confirmingLogout} onclick={logOutWfm} disabled={loggingOut}>{loggingOut ? 'Logging out…' : confirmingLogout ? 'Confirm log out' : 'Log out'}</button>
            {#if confirmingLogout}<button class="btn ghost" onclick={() => { confirmingLogout = false; logoutError = ''; }} disabled={loggingOut}>Cancel</button>{/if}
          {/if}
        </div>
      </div>
      {#if logoutError}<p class="error inset" role="alert">Couldn’t log out: {logoutError}</p>{/if}
      {#if wfmStatus?.logged_in || wfmStatus?.unlocked}<p class="exp inset">Logging out removes the encrypted login saved on this device, forgets its remembered unlock key, and discards any interrupted local listing batch. Your listings on warframe.market are not changed.</p>{/if}
    </section>
    {#if transport}<UsageSettings {transport} />{/if}
    <section class="wrap tw" aria-labelledby="set-updates">
      <div class="rail"><h3 id="set-updates">Updates</h3></div>
      <div class="ui-setting-row">
        <div class="ui-setting-copy"><strong>Application updates</strong><p>On Windows and Linux AppImage, TennoWorth also checks every 30 minutes while it is running. Updates are downloaded and installed only after you confirm.</p></div>
        <div class="ui-setting-control">
          <button class="btn" onclick={checkForUpdates} disabled={checkingUpdate}>{checkingUpdate ? 'Checking…' : 'Check for updates'}</button>
          {#if checkedUpdate?.available}<span class="status">Version {checkedUpdate.version} is available.</span>
          {:else if checkedUpdate?.checked && checkedUpdate.support === 'supported'}<span class="status">You’re up to date · v{checkedUpdate.current_version}</span>
          {:else if checkedUpdate?.checked && checkedUpdate.support === 'appimage_required'}<span class="status">This install can’t update itself. Download and run the TennoWorth AppImage to receive updates.</span>
          {:else if checkedUpdate?.checked && checkedUpdate.support === 'disabled_test_build'}<span class="status">Updates are disabled in this test build.</span>{/if}
        </div>
      </div>
      {#if updateError}<p class="error inset" role="alert">{updateError}</p>{/if}
    </section>
    <section class="wrap tw" aria-labelledby="set-relic-overlay">
      <div class="rail"><h3 id="set-relic-overlay">Relic reward overlay</h3></div>
      {#if overlay}
        <div class="ui-setting-row">
          <div class="ui-setting-copy"><label for="overlay-enabled">Enable local screen recognition</label><p>Captures only after a reward event or your retry shortcut. Frames stay in memory and are never uploaded.</p></div>
          <label class="ui-setting-check"><input id="overlay-enabled" type="checkbox" checked={overlay.enabled} disabled={savingOverlay} onchange={(event) => saveOverlay({ ...overlay!, enabled: event.currentTarget.checked })}><span>Enabled</span></label>
        </div>
        <div class="ui-section-group">
          <h4>Detection</h4>
          <div class="ui-setting-row">
            <div class="ui-setting-copy"><label for="overlay-auto">Automatic reward detection</label><p>Watches EE.log for “Got rewards”; the hotkey remains available when the game delays that line.</p></div>
            <label class="ui-setting-check"><input id="overlay-auto" type="checkbox" checked={overlay.autoDetect} disabled={!overlay.enabled || savingOverlay} onchange={(event) => saveOverlay({ ...overlay!, autoDetect: event.currentTarget.checked })}><span>Enabled</span></label>
          </div>
          <div class="ui-setting-row"><div class="ui-setting-copy"><label for="overlay-shortcut">Retry shortcut</label></div><div class="ui-setting-control"><input id="overlay-shortcut" class="ui-input shortcut" value={overlay.shortcut} disabled={!overlay.enabled || savingOverlay} onblur={(event) => saveOverlay({ ...overlay!, shortcut: event.currentTarget.value })}></div></div>
        </div>
        <div class="ui-section-group">
          <h4>Reward cards</h4>
          <div class="ui-setting-row"><div class="ui-setting-copy"><label for="overlay-scale">Card scale</label></div><div class="ui-setting-control"><input id="overlay-scale" type="range" min="0.75" max="1.5" step="0.05" value={overlay.scale} disabled={!overlay.enabled || savingOverlay} onchange={(event) => saveOverlay({ ...overlay!, scale: Number(event.currentTarget.value) })}><output class="mono" for="overlay-scale">{Math.round(overlay.scale * 100)}%</output></div></div>
          <div class="ui-setting-row"><div class="ui-setting-copy"><label for="overlay-prices">Live online asks</label><p>Replace cached prices with live online asks.</p></div><label class="ui-setting-check"><input id="overlay-prices" type="checkbox" checked={overlay.livePrices} disabled={!overlay.enabled || savingOverlay} onchange={(event) => saveOverlay({ ...overlay!, livePrices: event.currentTarget.checked })}><span>Enabled</span></label></div>
          <div class="ui-setting-row"><div class="ui-setting-copy"><label for="overlay-owned">Owned count</label><p>Show count from the latest inventory scan.</p></div><label class="ui-setting-check"><input id="overlay-owned" type="checkbox" checked={overlay.showOwned} disabled={!overlay.enabled || savingOverlay} onchange={(event) => saveOverlay({ ...overlay!, showOwned: event.currentTarget.checked })}><span>Shown</span></label></div>
        </div>
        <div class="ui-section-group">
          <h4>Diagnostics</h4>
          <div class="ui-setting-row"><div class="ui-setting-copy"><label for="overlay-diagnostics">Save local recognition diagnostics</label><p>Keep captures on this device to investigate recognition problems.</p></div><label class="ui-setting-check"><input id="overlay-diagnostics" type="checkbox" checked={overlay.diagnostics} disabled={!overlay.enabled || savingOverlay} onchange={(event) => saveOverlay({ ...overlay!, diagnostics: event.currentTarget.checked })}><span>Enabled</span></label></div>
          {#if overlay.diagnostics}<div class="diagnostics"><p class="warning">Diagnostic captures may contain player or game information. They stay on this device and are never uploaded automatically.</p><div class="ui-toolbar"><button class="btn" onclick={() => diagnosticsAction('open')}>Open diagnostics</button><button class="btn" onclick={() => diagnosticsAction('clear')}>Clear diagnostics</button></div></div>{/if}
        </div>
        <div class="ui-panel-footer">
          <div class="ui-toolbar"><button class="btn" onclick={previewOverlay} disabled={!overlay.enabled || savingOverlay}>Preview overlay</button><button class="btn" onclick={testOverlay} disabled={!overlay.enabled || savingOverlay}>Scan reward screen now</button>{#if overlayStatus}<span class="status" class:good={['watching', 'showing'].includes(overlayStatus.state)}>{overlayStatus.state.replaceAll('-', ' ')} · {overlayStatus.ocrReady ? 'OCR ready' : 'OCR unavailable'}</span>{/if}</div>
          {#if overlayStatus?.lastRun}<p class="exp">Last run: {overlayStatus.lastRun.outcome} · {overlayStatus.lastRun.recognizedSlots}/{overlayStatus.lastRun.expectedSlots || '?'} slots · {overlayStatus.lastRun.timings.totalMs} ms</p>{/if}
          <details class="capture-details"><summary>Capture status and display requirements</summary>{#if overlayStatus}<p class="status">{overlayStatus.backend} capture · {overlayStatus.presentationBackend} display</p>{/if}<p class="exp">Use Borderless Fullscreen or Windowed mode. Windows and X11 use direct window capture; Wayland captures through XWayland and presents through layer-shell when the compositor supports it. No interaction, injection, or automatic reward selection is performed.</p></details>
        </div>
      {:else}<p class="exp inset">Loading overlay settings…</p>{/if}
      {#if overlayError}<p class="error inset" role="alert">{overlayError}</p>{/if}
    </section>
    <NotificationSettings />
  {/if}
</div>

<style>
  .settings { display: flex; flex-direction: column; gap: var(--s4); }
  .lede { margin: 0; color: var(--muted); }
  .inset { padding: 0 var(--inset) var(--s4); }
  .exp { margin: 0; font-size: var(--text-control); line-height: var(--leading-body); color: var(--muted); max-width: 85ch; }
  .shortcut { width: 100%; font-family: var(--font-mono); }
  input[type='range'] { width: 8rem; min-height: var(--ctl-lg); accent-color: var(--accent); }
  .mono, .status { font-family: var(--font-mono); font-size: var(--text-caption); color: var(--muted); }
  .status { overflow-wrap: anywhere; }
  .good { color: var(--good); }
  .diagnostics { padding: var(--s3) var(--inset) 0; display: flex; flex-direction: column; gap: var(--s3); }
  .warning { margin: 0; padding-left: var(--s3); border-left: 2px solid var(--warn); color: var(--warn); font-size: var(--text-control); line-height: var(--leading-body); }
  .error { margin: 0; color: var(--bad); font-size: var(--text-control); }
  .ui-panel-footer { display: flex; flex-direction: column; gap: var(--s3); }
  .capture-details summary { cursor: pointer; min-height: var(--ctl); color: var(--muted); }
  .capture-details p { margin-top: var(--s2); }
</style>
