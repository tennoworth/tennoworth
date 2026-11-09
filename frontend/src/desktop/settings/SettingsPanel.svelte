<script lang="ts">
  import { busy as pending, roving } from '../../ui/keyboard-actions';
  import UsageSettings from './UsageSettings.svelte';
  import PriceReportSettings from './PriceReportSettings.svelte';
  import NotificationSettings from '../notifications/NotificationSettings.svelte';
  import ThemeSwitcher from '../../ui/ThemeSwitcher.svelte';
  import Select from '../../ui/Select.svelte';
  import AppIconSetting from './AppIconSetting.svelte';
  import { useDisplay } from '../../ui/display.svelte';
  import PresenceSettings from '../presence/PresenceSettings.svelte';
  import PresenceMark from '../presence/PresenceMark.svelte';
  import type { PresenceController } from '../presence/presence.svelte';
  import { presenceLine, presenceMark, presenceWord } from '../presence/presence';
  import type { AppIconController } from './app-icon.svelte';
  import { untrack, onMount, tick } from 'svelte';
  import type { ThemeController } from '../../ui/theme';
  import type { DesktopWfmStatus, DesktopCapabilities } from '../../contracts/desktop';
  import { AUTO_SCAN_CADENCE_CHOICES, type AutoScanSettings, type AutoScanStatus } from '../../contracts/desktop';
  import type { AutoScanController } from '../inventory/auto-scan.svelte';
  import { OVERLAY_LANGUAGES, type OverlayLanguage, type OverlaySettings, type OverlayStatus } from '../../contracts/data';
  import { LATEST_RELEASE_URL, type UpdateController } from '../updates/update-controller.svelte';
  import { humanError } from '../../contracts/errors';
  import { formatLocale, i18n, t } from '../../i18n/i18n.svelte';
  import { LANGUAGES, type LanguageCode } from '../../i18n/languages';

  interface Props {
    /** The boot-time controller from src/lib/theme.ts. */
    theme: ThemeController;
    /** Desktop only. Shared with the update banner, so either can install. */
    updates?: UpdateController;
    onwhatsnew?: () => void;
    transport?: DesktopCapabilities;
    /** Desktop only. The shell owns the instance, so the panel and the shell
     *  read one source of truth for these preferences and this status. */
    autoScan?: AutoScanController;
    /** Desktop only: the window and tray icon colour. */
    appIcon?: AppIconController;
    /** Desktop only: the user's warframe.market status. */
    presence?: PresenceController;
    wfmStatus?: DesktopWfmStatus | null;
    onwfmlogout?: () => Promise<void>;
    /** Opens the page at this section (the inbox's "Notification settings",
     *  the price sharing prompt's "What is sent"). */
    section?: 'notifications' | 'price-sharing' | 'account' | 'scan' | 'overlay' | 'app' | null;
    /** Whether Settings is on screen; it stays mounted while hidden. */
    active?: boolean;
    onsectionshown?: () => void;
    /** Desktop only: switches and saves the app language. */
    onlanguage?: (code: LanguageCode) => Promise<void>;
  }
  let { theme, updates, onwhatsnew, transport, autoScan, appIcon, presence, wfmStatus = null, onwfmlogout, section = null, onsectionshown, active = true, onlanguage }: Props = $props();
  const display = useDisplay();

  let overlay = $state<OverlaySettings | null>(null);
  let overlayStatus = $state<OverlayStatus | null>(null);
  let overlayError = $state('');
  let savingOverlay = $state(false);
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
  let sectionGeneration = 0;
  async function showSection(loaded: Promise<unknown>, wanted: NonNullable<Props['section']>, generation: number) {
    await loaded;
    await tick();
    if (disposed || !active || section !== wanted || generation !== sectionGeneration) return;
    const target = document.getElementById(`settings-${wanted}`);
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

  // Settings stays mounted after the first visit, so a later deep link (the
  // inbox's settings link, the account shortcut) arrives as a prop change and
  // is shown once the pane is on screen.
  let loaded: Promise<unknown> = Promise.resolve();
  let mounted = $state(false);
  $effect(() => {
    const wanted = section;
    const onScreen = active;
    const ready = mounted;
    untrack(() => {
      const generation = ++sectionGeneration;
      if (ready && wanted && onScreen) void showSection(loaded, wanted, generation);
    });
  });

  onMount(() => {
    if (!transport) { mounted = true; return; }
    const initial = () => Promise.all([transport.getOverlaySettings(), refreshOverlayStatus()])
      .then(([settings]) => { if (!disposed) { overlay = settings; troubleshootingOpen = settings.diagnostics; } })
      .catch((error) => { if (!disposed) overlayError = String(error); });
    const refreshStatus = async () => {
      if (!overlay || statusPolling || savingOverlay || disposed) return;
      statusPolling = true;
      try { await Promise.allSettled([refreshOverlayStatus(), autoScan?.refreshStatus()]); }
      // A failed read retries next time; both reads must finish first.
      finally { statusPolling = false; }
    };
    loaded = initial();
    mounted = true;
    const timer = window.setInterval(() => {
      // Native status reads only while the page is on screen.
      if (!active || document.hidden) return;
      void refreshStatus();
    }, 1000);
    const onVisibility = () => { if (active && !document.hidden) void refreshStatus(); };
    document.addEventListener('visibilitychange', onVisibility);
    return () => { disposed = true; window.clearInterval(timer); document.removeEventListener('visibilitychange', onVisibility); };
  });

  /** What the background loop is doing right now, in the user's terms. */
  function autoScanStatusText(status: AutoScanStatus | null): string {
    if (!status) return t('settings.scan.reading');
    if (!status.enabled) return t('settings.scan.manual');
    if (status.held) return t('settings.scan.held');
    if (status.lastError) return t('settings.scan.failed', { error: status.lastError });
    if (!status.gameRunning) return t('settings.scan.waiting');
    const last = status.lastScanAt ? t('settings.scan.last', { time: new Date(status.lastScanAt * 1000).toLocaleTimeString(formatLocale()) }) : '';
    const next = status.nextCheckAt ? t('settings.scan.next', { time: new Date(status.nextCheckAt * 1000).toLocaleTimeString(formatLocale()) }) : '';
    return [last, next].filter(Boolean).join(' ') || t('settings.scan.watching');
  }

  function autoScanSummary(): string {
    const settings = autoScan?.settings;
    if (!settings) return t('settings.loading');
    return settings.enabled ? t('settings.onEvery', { minutes: settings.cadenceMinutes }) : t('settings.off');
  }

  // Each language under its own name, the way language menus list them.
  let rewardLanguageChoices = $derived<{ value: OverlayLanguage; label: string }[]>([
    { value: 'auto', label: t('settings.sameAsGame') },
    ...OVERLAY_LANGUAGES.map((code) => ({ value: code, label: languageName(code) })),
  ]);
  const appLanguageChoices = LANGUAGES.filter((language) => language.offered).map((language) => ({ value: language.code as LanguageCode, label: language.native }));
  let switchingLanguage = $state(false);
  let languageFailed = $state(false);
  async function switchLanguage(code: LanguageCode) {
    switchingLanguage = true;
    languageFailed = false;
    try { await onlanguage?.(code); } catch (error) { console.error(error); languageFailed = true; } finally { switchingLanguage = false; }
  }
  function languageName(code: string): string {
    return LANGUAGES.find((language) => language.code === code)?.native ?? code;
  }

  function overlaySummary(): string {
    if (!overlay) return t('settings.loading');
    if (!overlay.enabled) return t('settings.off');
    if (!overlayStatus) return t('settings.on');
    return overlayStateText(overlayStatus);
  }

  function overlayStateText(status: OverlayStatus): string {
    return `${t(`settings.overlay.${status.state}`)} · ${t(status.ocrReady ? 'settings.ocrReady' : 'settings.ocrUnavailable')}`;
  }

  function lastRunText(): string {
    const run = overlayStatus?.lastRun;
    return run ? t('settings.lastRun', { outcome: run.outcome, found: run.recognizedSlots, expected: run.expectedSlots || '?', ms: run.timings.totalMs }) : t('settings.noRunYet');
  }

  function sessionText(): string {
    if (!wfmStatus) return t('settings.checkingSession');
    if (wfmStatus.unlocked) return t('settings.unlocked');
    if (wfmStatus.logged_in) return t('settings.locked');
    return t('settings.signedOut');
  }

  // `revert` puts the control back to the stored value. A failed save leaves the
  // settings unchanged, so nothing re-renders and the control would keep showing
  // a value that was never stored - and the next click would send its opposite.
  let cadenceOptions = $derived(AUTO_SCAN_CADENCE_CHOICES.map((minutes) => ({ value: minutes, label: t('settings.everyMinutes', { minutes }) })));

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
  <h2>{t('settings.title')}</h2>
  <p class="lede">{t('settings.lede')}</p>
</section>

<div class="settings ui-reading">
  <!-- Most visits ask "is it running?", so the answers lead and link to the
       controls that change them. -->
  <div class="ui-summary-strip glance" aria-label={t('settings.currentState')}>
    {#if autoScan}<a href="#settings-scan"><span class="k">{t('settings.autoScan')}</span><strong>{autoScanSummary()}</strong><span>{autoScanStatusText(autoScan.status)}</span></a>{/if}
    {#if transport}<a href="#settings-overlay"><span class="k">{t('settings.relicOverlay')}</span><strong>{overlaySummary()}</strong><span>{overlay?.enabled ? lastRunText() : t('settings.notReading')}</span></a>{/if}
    {#if presence?.status?.signedIn}
      <!-- What buyers see is the everyday question; the session is its second line. -->
      <a href="#settings-account"><span class="k">warframe.market</span><strong class="with-mark"><PresenceMark kind={presenceMark(presence.status)} />{presenceWord(presence.status)}</strong><span>{t('settings.sessionUnlocked', { line: presenceLine(presence.status) })}</span></a>
    {:else}
      <a href="#settings-account"><span class="k">warframe.market</span><strong class:ok={wfmStatus?.unlocked}>{sessionText()}</strong><span>{t('settings.encryptedLogin')}</span></a>
    {/if}
  </div>
  <!-- Only sections the cards above do not already link to: each extra link
       was another Tab stop to the same place. -->
  <nav class="jump" aria-label={t('settings.sections')}>
    <span class="k">{t('settings.everyday')}</span>
    {#if !transport}<a href="#settings-overlay">{t('settings.relicOverlay')}</a>{/if}
    <a href="#settings-notifications">{t('settings.notifications')}</a>
    <span class="k">{t('settings.setOnce')}</span>
    <a href="#settings-app">{t('settings.appLink')}</a>
  </nav>

  <p class="tier">{t('settings.everyday')}</p>
  {#if autoScan}
    <section class="wrap tw" id="settings-scan" tabindex="-1" aria-labelledby="set-auto-scan">
      <div class="rail"><h3 id="set-auto-scan">{t('settings.autoScan')}</h3></div>
      {#if autoScan.settings}
        <div class="ui-setting-row">
          <div class="ui-setting-copy"><strong>{t('settings.status')}</strong><p>{autoScanStatusText(autoScan.status)}</p></div>
        </div>
        <div class="ui-setting-row">
          <div class="ui-setting-copy"><label for="auto-scan-enabled">{t('settings.scanAuto')}</label><p>{t('settings.scanAutoHelp')}</p></div>
          <div class="ui-setting-control">
            <label class="ui-setting-check"><input id="auto-scan-enabled" type="checkbox" checked={autoScan.settings.enabled} use:pending={savingAutoScan} onchange={(event) => { const input = event.currentTarget; void saveAutoScan({ ...autoScan!.settings!, enabled: input.checked }, (stored) => (input.checked = stored.enabled)); }}><span>{t('settings.enabled')}</span></label>
            {#if autoScan.settings.enabled}
              <Select id="auto-scan-cadence" label={t('settings.scanEvery')} value={autoScan.settings.cadenceMinutes} options={cadenceOptions} disabled={savingAutoScan} onchange={(minutes) => void saveAutoScan({ ...autoScan!.settings!, cadenceMinutes: minutes }, () => {})} />
            {/if}
          </div>
        </div>
        <div class="ui-setting-row">
          <div class="ui-setting-copy"><label for="auto-scan-adopt">{t('settings.adopt')}</label><p>{t('settings.adoptHelp')}</p></div>
          <label class="ui-setting-check"><input id="auto-scan-adopt" type="checkbox" checked={autoScan.settings.adoptAutomatically} use:pending={savingAutoScan} onchange={(event) => { const input = event.currentTarget; void saveAutoScan({ ...autoScan!.settings!, adoptAutomatically: input.checked }, (stored) => (input.checked = stored.adoptAutomatically)); }}><span>{t('settings.automatic')}</span></label>
        </div>
      {:else}<p class="exp inset">{t('settings.loadingScan')}</p>{/if}
      {#if autoScan.settingsError}<p class="error inset" role="alert">{autoScan.settingsError}</p>{/if}
    </section>
  {/if}
  <section class="wrap tw" id="settings-overlay" tabindex="-1" aria-labelledby="set-relic-overlay">
    <div class="rail">
      <h3 id="set-relic-overlay">{t('settings.overlayTitle')}</h3>
      {#if overlay}<button class="btn xs" onclick={previewOverlay} disabled={!overlay.enabled} use:pending={savingOverlay}>{t('settings.previewOverlay')}</button><button class="btn xs" onclick={testOverlay} disabled={!overlay.enabled} use:pending={savingOverlay}>{t('settings.scanRewards')}</button>{/if}
    </div>
    {#if overlay}
      <div class="ui-setting-row">
        <div class="ui-setting-copy"><label for="overlay-enabled">{t('settings.recognition')}</label><p>{t('settings.recognitionHelp')}</p>{#if overlay.enabled && overlayStatus?.lastRun}<p>{lastRunText()}</p>{/if}</div>
        <div class="ui-setting-control">
          <label class="ui-setting-check"><input id="overlay-enabled" type="checkbox" checked={overlay.enabled} use:pending={savingOverlay} onchange={(event) => saveOverlay({ ...overlay!, enabled: event.currentTarget.checked })}><span>{t('settings.enabled')}</span></label>
          {#if overlayStatus}<span class="status" class:good={['watching', 'showing'].includes(overlayStatus.state)}>{overlayStateText(overlayStatus)}</span>{/if}
        </div>
      </div>
      <div class="split">
        <div class="ui-section-group">
          <h4>{t('settings.detection')}</h4>
          <div class="ui-setting-row">
            <div class="ui-setting-copy"><label for="overlay-auto">{t('settings.autoDetect')}</label><p>{t('settings.autoDetectHelp')}</p></div>
            <label class="ui-setting-check"><input id="overlay-auto" type="checkbox" checked={overlay.autoDetect} disabled={!overlay.enabled} use:pending={savingOverlay} onchange={(event) => saveOverlay({ ...overlay!, autoDetect: event.currentTarget.checked })}><span>{t('settings.enabled')}</span></label>
          </div>
          <div class="ui-setting-row"><div class="ui-setting-copy"><label for="overlay-shortcut">{t('settings.retryShortcut')}</label><p>{t('settings.retryShortcutHelp')}</p></div><div class="ui-setting-control"><input id="overlay-shortcut" class="ui-input shortcut" value={overlay.shortcut} disabled={!overlay.enabled} use:pending={savingOverlay} onblur={(event) => saveOverlay({ ...overlay!, shortcut: event.currentTarget.value })}></div></div>
          <div class="ui-setting-row">
            <div class="ui-setting-copy">
              <label id="overlay-language-label" for="overlay-language">{t('settings.rewardLanguage')}</label>
              <p>{t('settings.rewardLanguageHelp')}</p>
              {#if overlay.enabled && overlayStatus}<p>{overlayStatus.ocrLanguageNote ?? t('settings.rewardLanguageNow', { language: languageName(overlayStatus.ocrLanguage) })}</p>{/if}
            </div>
            <div class="ui-setting-control"><Select id="overlay-language" labelledby="overlay-language-label" value={overlay.ocrLanguage} options={rewardLanguageChoices} disabled={!overlay.enabled || savingOverlay} onchange={(choice) => saveOverlay({ ...overlay!, ocrLanguage: choice })} /></div>
          </div>
        </div>
        <div class="ui-section-group">
          <h4>{t('settings.rewardCards')}</h4>
          <div class="ui-setting-row"><div class="ui-setting-copy"><label for="overlay-scale">{t('settings.cardScale')}</label></div><div class="ui-setting-control"><input id="overlay-scale" type="range" min="0.75" max="1.5" step="0.05" value={overlay.scale} disabled={!overlay.enabled} use:pending={savingOverlay} onchange={(event) => saveOverlay({ ...overlay!, scale: Number(event.currentTarget.value) })}><output class="mono" for="overlay-scale">{Math.round(overlay.scale * 100)}%</output></div></div>
          <div class="ui-setting-row"><div class="ui-setting-copy"><label for="overlay-prices">{t('settings.liveAsks')}</label><p>{t('settings.liveAsksHelp')}</p></div><label class="ui-setting-check"><input id="overlay-prices" type="checkbox" checked={overlay.livePrices} disabled={!overlay.enabled} use:pending={savingOverlay} onchange={(event) => saveOverlay({ ...overlay!, livePrices: event.currentTarget.checked })}><span>{t('settings.enabled')}</span></label></div>
          <div class="ui-setting-row"><div class="ui-setting-copy"><label for="overlay-owned">{t('settings.ownedCount')}</label><p>{t('settings.ownedCountHelp')}</p></div><label class="ui-setting-check"><input id="overlay-owned" type="checkbox" checked={overlay.showOwned} disabled={!overlay.enabled} use:pending={savingOverlay} onchange={(event) => saveOverlay({ ...overlay!, showOwned: event.currentTarget.checked })}><span>{t('settings.shown')}</span></label></div>
        </div>
      </div>
      <details class="troubleshooting" bind:open={troubleshootingOpen}>
        <summary>{t('settings.troubleshooting')}</summary>
        <div class="ui-setting-row"><div class="ui-setting-copy"><label for="overlay-diagnostics">{t('settings.diagnostics')}</label><p>{t('settings.diagnosticsHelp')}</p></div><label class="ui-setting-check"><input id="overlay-diagnostics" type="checkbox" checked={overlay.diagnostics} disabled={!overlay.enabled} use:pending={savingOverlay} onchange={(event) => saveOverlay({ ...overlay!, diagnostics: event.currentTarget.checked })}><span>{t('settings.enabled')}</span></label></div>
        {#if overlay.diagnostics}<div class="diagnostics"><p class="warning">{t('settings.diagnosticsWarning')}</p><div class="ui-toolbar"><button class="btn" onclick={() => diagnosticsAction('open')}>{t('settings.openDiagnostics')}</button><button class="btn" onclick={() => diagnosticsAction('clear')}>{t('settings.clearDiagnostics')}</button></div></div>{/if}
        <div class="requirements">
          {#if overlayStatus}<p class="status">{t('settings.backends', { capture: overlayStatus.backend, display: overlayStatus.presentationBackend })}</p>{/if}
          <p class="exp">{t('settings.requirements')}</p>
        </div>
      </details>
    {:else}<p class="exp inset">{t('settings.loadingOverlay')}</p>{/if}
    {#if overlayError}<p class="error inset" role="alert">{overlayError}</p>{/if}
  </section>
  <NotificationSettings />

  <p class="tier">{t('settings.setOnce')}</p>
  <section class="wrap tw" id="settings-account" tabindex="-1" aria-labelledby="set-wfm-account">
    <div class="rail"><h3 id="set-wfm-account">{t('settings.account')}</h3></div>
    <div class="ui-setting-row">
      <div class="ui-setting-copy"><strong>{t('settings.session')}</strong><p>{t(wfmStatus?.logged_in || wfmStatus?.unlocked ? 'settings.logoutHelp' : 'settings.manageLogin')}</p></div>
      <div class="ui-setting-control">
        <span class="status" class:ok={wfmStatus?.unlocked}>{sessionText()}</span>
        {#if wfmStatus?.logged_in || wfmStatus?.unlocked}
          <button class="btn" class:bad={confirmingLogout} onclick={logOutWfm} use:pending={loggingOut}>{t(loggingOut ? 'settings.loggingOut' : confirmingLogout ? 'settings.confirmLogout' : 'settings.logout')}</button>
          {#if confirmingLogout}<button class="btn ghost" onclick={() => { confirmingLogout = false; logoutError = ''; }} use:pending={loggingOut}>{t('settings.cancel')}</button>{/if}
        {/if}
      </div>
    </div>
    {#if logoutError}<p class="error inset" role="alert">{t('settings.logoutFailed', { error: logoutError })}</p>{/if}
    {#if presence}<PresenceSettings {presence} />{/if}
  </section>
  <!-- Each of these is one row, set once. As three panels they used to lead the page. -->
  <section class="wrap tw" id="settings-app" tabindex="-1" aria-labelledby="set-app">
    <div class="rail"><h3 id="set-app">{t('settings.thisApp')}</h3></div>
    <div class="ui-section-group first">
      <h4>{t('settings.appearance')}</h4>
      {#if onlanguage}
        <div class="ui-setting-row">
          <div class="ui-setting-copy"><strong id="app-language-label">{t('settings.appLanguage')}</strong><p>{t('settings.appLanguageHelp')}</p>
            {#if languageFailed}<p class="ui-notice" data-tone="bad" role="alert">{t('settings.appLanguageFailed')}</p>{/if}</div>
          <div class="ui-setting-control"><Select id="app-language" labelledby="app-language-label" value={i18n.language as LanguageCode} options={appLanguageChoices} disabled={switchingLanguage} onchange={(code) => void switchLanguage(code)} /></div>
        </div>
      {/if}
      <div class="ui-setting-row">
        <div class="ui-setting-copy"><strong>{t('settings.colourMode')}</strong><p>{t('settings.colourModeHelp')}</p></div>
        <div class="ui-setting-control"><ThemeSwitcher {theme} label={t('settings.colourMode')} /></div>
      </div>
      {#if appIcon}<AppIconSetting {appIcon} />{/if}
      {#if display}
        <div class="ui-setting-row">
          <div class="ui-setting-copy"><strong>{t('settings.itemLists')}</strong><p>{t('settings.itemListsHelp')}</p><p class="credit">{t('settings.credit')}</p></div>
          <div class="ui-setting-control">
            <div class="ui-segmented" role="group" aria-label={t('settings.itemLists')} use:roving>
              <button type="button" aria-pressed={display.value === 'visual'} onclick={() => display.set('visual')}>{t('settings.visual')}</button>
              <button type="button" aria-pressed={display.value === 'compact'} onclick={() => display.set('compact')}>{t('settings.compact')}</button>
            </div>
          </div>
        </div>
      {/if}
    </div>
    <div class="ui-section-group">
      <h4>{t('settings.updates')}</h4>
      <div class="ui-setting-row">
        <div class="ui-setting-copy"><strong>{t('settings.appUpdates')}</strong><p>{t('settings.appUpdatesHelp')}</p></div>
        <div class="ui-setting-control">
          {#if onwhatsnew}<button class="btn" onclick={onwhatsnew}>{t('settings.whatsNew')}</button>{/if}
          {#if updates}
            {@const info = updates.info}
            {#if updates.installed}
              <button class="btn primary" onclick={() => updates.restart()}>{t('settings.restart')}</button>
            {:else if info?.available}
              <button class="btn primary" onclick={() => updates.install()} use:pending={updates.installing || updates.checking}>{t(updates.installing ? 'settings.installing' : 'settings.install')}</button>
            {:else}
              <button class="btn" onclick={() => updates.check()} use:pending={updates.checking}>{t(updates.checking ? 'settings.checking' : 'settings.checkUpdates')}</button>
            {/if}
            <span class="status" role="status">
              {#if updates.installed}{t('settings.update.installed', { version: info?.version ?? '' })}
              {:else if updates.installing}{t('settings.update.installing', { version: info?.version ?? '' })}
              {:else if info?.available}{t('settings.update.available', { version: info.version ?? '', current: info.current_version })}
              {:else if info?.checked && info.support === 'supported'}{t('settings.update.current', { current: info.current_version })}
              {:else if info?.checked && info.support === 'appimage_required'}{t('settings.update.appimage')}
              {:else if info?.checked && info.support === 'disabled_test_build'}{t('settings.update.disabled')}{/if}
            </span>
          {/if}
        </div>
      </div>
      {#if updates?.linuxRequirement}
        {@const requirement = updates.linuxRequirement}
        <p class="warning manual" data-testid="settings-linux-requirement">{[t('settings.linuxRequirement', { required: requirement.glibc_required }), requirement.glibc_found ? t('settings.linuxRequirementFound', { found: requirement.glibc_found }) : ''].filter(Boolean).join(' ')}</p>
      {/if}
      {#if updates?.needsManualInstall}
        <p class="warning manual" data-testid="settings-update-manual-install">{t('settings.manualBefore')}<a href={LATEST_RELEASE_URL} target="_blank" rel="noopener noreferrer">{t('settings.latestRelease')}</a>{t('settings.manualAfter')}</p>
      {/if}
      {#if updates?.error}<p class="error inset" role="alert">{updates.error}</p>{/if}
    </div>
    {#if transport}<UsageSettings {transport} /><PriceReportSettings {transport} />{/if}
  </section>
</div>

<style>
  .settings { display: flex; flex-direction: column; gap: var(--s4); }
  .lede { margin: 0; color: var(--muted); }
  .credit { font-size: var(--text-caption); }
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
  .warning.manual { margin: var(--s3) var(--inset) 0; }

  /* The strip holds one to three cells, depending on what this build offers. */
  .glance { grid-template-columns: repeat(auto-fit, minmax(min(12rem, 100%), 1fr)); }
  .glance > a { overflow-wrap: anywhere; display: flex; flex-direction: column; gap: var(--s1); padding: var(--s3) var(--inset); min-width: 0; color: var(--fg); border-bottom: 0; }
  .glance > a + a { border-left: 1px var(--rule) var(--border); }
  .glance > a:hover { background: var(--panel-2); }
  .glance strong { font: 500 var(--text-body)/var(--leading-control) var(--font-body); overflow-wrap: anywhere; }
  .glance strong.with-mark { display: flex; align-items: center; gap: var(--s2); }
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
