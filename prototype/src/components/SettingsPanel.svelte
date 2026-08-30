<script lang="ts">
  // The Settings view. Home for preferences that are not part of doing the
  // work - starting with Appearance, which is where the theme control lives
  // now that it is no longer chrome on the landing header and the sidebar.
  //
  // Structure: one `.wrap.tw` panel per section, each with a `.rail` title and
  // a `.sbody`. Adding a section = adding another panel; nothing here is
  // special-cased to Appearance.
  import ThemeSwitcher from './ThemeSwitcher.svelte';
  import { onMount } from 'svelte';
  import type { ThemeController } from '../lib/theme';
  import type { DesktopWfmStatus, Transport } from '../lib/transport';
  import type { OverlaySettings, OverlayStatus } from '../lib/types';
  import { checkUpdate, type UpdateStatus } from '../lib/desktop-update';
  import { humanError } from '../lib/errors';

  interface Props {
    /** The boot-time controller from src/lib/theme.ts. */
    theme: ThemeController;
    transport?: Transport;
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
  <span
    class="lede-dot"
    role="img"
    aria-label="About this view"
    title="Preferences for this install. They persist on this machine and are never uploaded."
  >ⓘ</span>
</section>

<div class="settings">
  <section class="wrap tw" aria-labelledby="set-appearance">
    <div class="rail"><h3 id="set-appearance">Appearance</h3></div>
    <div class="sbody">
      <div class="field">
        <span class="k">Colour mode</span>
        <ThemeSwitcher {theme} label="Colour mode" />
      </div>
      <p class="exp">
        System follows your operating system's light/dark setting and changes
        with it; Light and Dark pin the app regardless.
      </p>
    </div>
  </section>

  {#if isDesktop}
    <section class="wrap tw" aria-labelledby="set-wfm-account">
      <div class="rail"><h3 id="set-wfm-account">warframe.market account</h3></div>
      <div class="sbody">
        <div class="overlay-actions">
          <span class="status">
            {#if !wfmStatus}Checking session…
            {:else if wfmStatus.unlocked}Signed in · session unlocked
            {:else if wfmStatus.logged_in}Signed in · session locked
            {:else}Not signed in
            {/if}
          </span>
          {#if wfmStatus?.logged_in || wfmStatus?.unlocked}
            <button class:danger={confirmingLogout} onclick={logOutWfm} disabled={loggingOut}>
              {loggingOut ? 'Logging out…' : confirmingLogout ? 'Confirm log out' : 'Log out'}
            </button>
            {#if confirmingLogout}
              <button class="ghost" onclick={() => { confirmingLogout = false; logoutError = ''; }} disabled={loggingOut}>Cancel</button>
            {/if}
          {/if}
        </div>
        {#if logoutError}<p class="error" role="alert">Couldn’t log out: {logoutError}</p>{/if}
        <p class="exp">Logging out removes the encrypted login saved on this device, forgets its remembered unlock key, and discards any interrupted local listing batch. Your listings on warframe.market are not changed.</p>
      </div>
    </section>

    <section class="wrap tw" aria-labelledby="set-updates">
      <div class="rail"><h3 id="set-updates">Updates</h3></div>
      <div class="sbody">
        <div class="overlay-actions">
          <button onclick={checkForUpdates} disabled={checkingUpdate}>
            {checkingUpdate ? 'Checking…' : 'Check for updates'}
          </button>
          {#if checkedUpdate?.available}
            <span class="status">Version {checkedUpdate.version} is available.</span>
          {:else if checkedUpdate?.checked && checkedUpdate.support === 'supported'}
            <span class="status">You’re up to date · v{checkedUpdate.current_version}</span>
          {:else if checkedUpdate?.checked && checkedUpdate.support === 'appimage_required'}
            <span class="status">This install can’t update itself. Download and run the TennoWorth AppImage to receive updates.</span>
          {:else if checkedUpdate?.checked && checkedUpdate.support === 'disabled_test_build'}
            <span class="status">Updates are disabled in this test build.</span>
          {/if}
        </div>
        {#if updateError}<p class="error" role="alert">{updateError}</p>{/if}
        <p class="exp">On Windows and Linux AppImage, TennoWorth also checks every 30 minutes while it is running. Updates are downloaded and installed only after you confirm.</p>
      </div>
    </section>

    <section class="wrap tw" aria-labelledby="set-relic-overlay">
      <div class="rail"><h3 id="set-relic-overlay">Relic reward overlay</h3></div>
      <div class="sbody">
        {#if overlay}
          <label class="check-row">
            <input
              type="checkbox"
              checked={overlay.enabled}
              disabled={savingOverlay}
              onchange={(event) => saveOverlay({ ...overlay!, enabled: event.currentTarget.checked })}
            >
            <span><strong>Enable local screen recognition</strong><small>Captures only after a reward event or your retry shortcut. Frames stay in memory and are never uploaded.</small></span>
          </label>
          <label class="check-row">
            <input type="checkbox" checked={overlay.autoDetect} disabled={!overlay.enabled || savingOverlay} onchange={(event) => saveOverlay({ ...overlay!, autoDetect: event.currentTarget.checked })}>
            <span><strong>Automatic reward detection</strong><small>Watches EE.log for “Got rewards”; the hotkey remains available when the game delays that line.</small></span>
          </label>
          <div class="field">
            <label class="k" for="overlay-shortcut">Retry shortcut</label>
            <input id="overlay-shortcut" class="text-input" value={overlay.shortcut} disabled={!overlay.enabled || savingOverlay} onblur={(event) => saveOverlay({ ...overlay!, shortcut: event.currentTarget.value })}>
          </div>
          <div class="field">
            <label class="k" for="overlay-scale">Card scale</label>
            <input id="overlay-scale" type="range" min="0.75" max="1.5" step="0.05" value={overlay.scale} disabled={!overlay.enabled || savingOverlay} onchange={(event) => saveOverlay({ ...overlay!, scale: Number(event.currentTarget.value) })}>
            <span class="mono">{Math.round(overlay.scale * 100)}%</span>
          </div>
          <label class="check-row compact"><input type="checkbox" checked={overlay.livePrices} disabled={!overlay.enabled || savingOverlay} onchange={(event) => saveOverlay({ ...overlay!, livePrices: event.currentTarget.checked })}><span>Replace cached prices with live online asks</span></label>
          <label class="check-row compact"><input type="checkbox" checked={overlay.showOwned} disabled={!overlay.enabled || savingOverlay} onchange={(event) => saveOverlay({ ...overlay!, showOwned: event.currentTarget.checked })}><span>Show count from the latest inventory scan</span></label>
          <label class="check-row compact"><input type="checkbox" checked={overlay.diagnostics} disabled={!overlay.enabled || savingOverlay} onchange={(event) => saveOverlay({ ...overlay!, diagnostics: event.currentTarget.checked })}><span>Save local recognition diagnostics</span></label>
          {#if overlay.diagnostics}
            <p class="warning">Diagnostic captures may contain player or game information. They stay on this device and are never uploaded automatically.</p>
            <div class="overlay-actions">
              <button onclick={() => diagnosticsAction('open')}>Open diagnostics</button>
              <button onclick={() => diagnosticsAction('clear')}>Clear diagnostics</button>
            </div>
          {/if}
          <div class="overlay-actions">
            <button onclick={previewOverlay} disabled={!overlay.enabled || savingOverlay}>Preview overlay</button>
            <button onclick={testOverlay} disabled={!overlay.enabled || savingOverlay}>Scan reward screen now</button>
            {#if overlayStatus}<span class="status"><i class="status-dot {overlayStatus.state}"></i>{overlayStatus.state.replaceAll('-', ' ')} · {overlayStatus.backend} capture · {overlayStatus.presentationBackend} display · {overlayStatus.ocrReady ? 'OCR ready' : 'OCR unavailable'}</span>{/if}
          </div>
          {#if overlayStatus?.lastRun}
            <p class="exp">Last run: {overlayStatus.lastRun.outcome} · {overlayStatus.lastRun.recognizedSlots}/{overlayStatus.lastRun.expectedSlots || '?'} slots · {overlayStatus.lastRun.timings.totalMs} ms</p>
          {/if}
          {#if overlayError}<p class="error" role="alert">{overlayError}</p>{/if}
          <p class="exp">Use Borderless Fullscreen or Windowed mode. Windows and X11 use direct window capture; Wayland captures through XWayland and presents through layer-shell when the compositor supports it. No interaction, injection, or automatic reward selection is performed.</p>
        {:else}
          <p class="exp">Loading overlay settings…</p>
        {/if}
      </div>
    </section>
  {/if}
</div>

<style>
  /* The view header + its info dot, as SellPane has them: both components
     render the shared markup, but the rules are Svelte-scoped per component,
     so each self-contained view carries its own copy. */
  .view-header {
    display: flex;
    align-items: center;
    gap: var(--s2);
    min-height: var(--rail);
    flex-wrap: wrap;
  }
  .view-header h2 {
    font-size: 20px;
    font-weight: 600;
    text-transform: none;
    letter-spacing: -0.01em;
    color: var(--fg);
    margin: 0;
    line-height: 1.5rem;
  }
  .lede-dot {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 16px;
    height: 16px;
    border-radius: 50%;
    font-size: 11px;
    line-height: 1;
    color: var(--muted);
    border: 1px var(--rule) var(--hairline);
    cursor: help;
  }
  .lede-dot:hover, .lede-dot:focus-visible { color: var(--accent); border-color: var(--accent); }

  .settings { display: flex; flex-direction: column; gap: var(--stack); max-width: 44rem; margin-top: var(--stack); }
  .sbody {
    display: flex;
    flex-direction: column;
    gap: var(--s2);
    padding: var(--s3) var(--inset) var(--s4);
  }
  .field { display: flex; align-items: center; gap: var(--s2) var(--s4); flex-wrap: wrap; }
  .field .k {
    width: 7rem;
    flex: 0 0 auto;
    font-family: var(--font-ui);
    font-size: 10px;
    line-height: 1rem;
    letter-spacing: 0.12em;
    text-transform: uppercase;
    font-weight: 600;
    color: var(--muted);
  }
  /* Helper copy is real information, so --muted (the readable floor), never
     --faint, which is decorative-only. */
  .sbody .exp { margin: 0; font-size: 12px; line-height: 1rem; color: var(--muted); max-width: 60ch; white-space: normal; }
  .check-row { display:flex; align-items:flex-start; gap:var(--s2); color:var(--fg); cursor:pointer; }
  .check-row input { margin-top:3px; accent-color:var(--accent); }
  .check-row span { display:flex; flex-direction:column; gap:2px; }
  .check-row small { color:var(--muted); max-width:62ch; }
  .check-row.compact { align-items:center; }
  .text-input { min-width:14rem; padding:6px 8px; border:1px var(--rule) var(--hairline); border-radius:4px; background:var(--panel); color:var(--fg); }
  input[type='range'] { width:min(18rem,55vw); accent-color:var(--accent); }
  .mono,.status { font-family:var(--font-mono); font-size:11px; color:var(--muted); }
  .overlay-actions { display:flex; align-items:center; gap:var(--s3); flex-wrap:wrap; }
  .status { display:flex; align-items:center; gap:6px; text-transform:capitalize; }
  .status-dot { width:7px; height:7px; border-radius:50%; background:var(--muted); }
  .status-dot.watching,.status-dot.showing { background:var(--good); }
  .status-dot.recognizing { background:var(--accent); }
  .status-dot.error { background:var(--bad); }
  .error { margin:0; color:var(--bad); font-size:12px; }
  .warning { margin:0; color:var(--warn, #b7791f); font-size:12px; line-height:1rem; }
  button.danger { color:var(--bad); border-color:var(--bad); }
</style>
