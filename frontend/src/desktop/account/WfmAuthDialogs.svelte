<script lang="ts">
  import { useDesktopServices } from '../shell/desktop-context';
  const { desktopWfmLogin, desktopWfmLoginCancel, desktopWfmLoginWithToken, desktopWfmUnlock, desktopTrySilentUnlock, desktopRememberAvailable } = useDesktopServices();
  import { humanError } from '../../contracts/errors';
  import WfmTokenGuide from './WfmTokenGuide.svelte';
  import Select from '../../ui/Select.svelte';

  import { t } from '../../i18n/i18n.svelte';

  const PLATFORMS = ['pc', 'ps4', 'xbox', 'switch'] as const;
  let platformOptions = $derived(PLATFORMS.map(value => ({ value, label: t(`auth.platform.${value}`) })));
  
import { DesktopCmdError } from '../../contracts/errors';

  let { onunlocked, onautherror, onreport }: {
    onunlocked: (next: string | null) => void;
    /** A sign-in or unlock failure worth a bug report, or null once one succeeds. */
    onautherror?: (error: unknown) => void;
    onreport?: () => void;
  } = $props();

  let wfmLoginDialog = $state<HTMLDialogElement>();
  let wfmUnlockDialog = $state<HTMLDialogElement>();
  let wfmLoginPassphrase = $state('');
  let wfmLoginConfirm = $state('');
  let wfmLoginPlatform = $state('pc');
  // 'token' is the fallback for when the sign-in window cannot work (it
  // crashes under some Linux WebKitGTK builds): the user signs in with their
  // own browser and pastes its JWT cookie.
  let wfmLoginMode = $state<'window' | 'token'>('window');
  let wfmLoginToken = $state('');
  let wfmUnlockPassphrase = $state('');
  // "Remember on this device" (OS keyring). One preference shared by the
  // login and unlock dialogs; default on - the browser-cookie parity call.
  let wfmRemember = $state(true);
  // null until asked. A desktop without a keyring service (KDE Wallet switched
  // off, no Secret Service) used to accept the tick and silently ask for the
  // passphrase again next launch.
  let rememberAvailable = $state<boolean | null>(null);

  async function checkRemember() {
    try {
      rememberAvailable = await desktopRememberAvailable();
    } catch {
      rememberAvailable = false;
    }
  }
  let wfmAuthBusy = $state(false);
  let wfmAuthError = $state<string | null>(null);
  // Wrong or mismatched passphrases and a closed window are the user's to fix, not a bug.
  let wfmAuthReportable = $state(false);
  // What to do once the session unlocks: 'list' re-opens the listing flow the
  // CTA started; null (the Resume path) leaves the caller where it was.
  // Threaded through to `onunlocked` rather than acted on here - App.svelte
  // owns what 'list' means (opening its own listingOpen state).
  let authNext: string | null = null;
  let wfmLoginPassInput: HTMLInputElement | undefined;
  let wfmUnlockPassInput: HTMLInputElement | undefined;

  function failed(err: unknown) {
    wfmAuthError = humanError(err);
    wfmAuthReportable = !(err instanceof DesktopCmdError && err.code === 'bad_passphrase');
    if (wfmAuthReportable) onautherror?.(err);
  }

  export async function open(code: string, next: string | null = null) {
    wfmAuthError = null;
    wfmAuthReportable = false;
    authNext = next;
    void checkRemember();
    if (code === 'needs_login') {
      wfmLoginPassphrase = '';
      wfmLoginConfirm = '';
      wfmLoginDialog?.showModal();
      wfmLoginPassInput?.focus();
    } else {
      // A remembered device key (OS keyring) unlocks without the modal. Any
      // miss - no entry, no keyring daemon, stale key - falls through to the
      // passphrase prompt exactly as before.
      try {
        if (await desktopTrySilentUnlock()) {
          unlocked();
          return;
        }
      } catch (e) {
        console.error('silent unlock failed', e);
      }
      wfmUnlockPassphrase = '';
      wfmUnlockDialog?.showModal();
      wfmUnlockPassInput?.focus();
    }
  }

  function unlocked() {
    onautherror?.(null);
    onunlocked(authNext);
    authNext = null;
  }

  // Forgot the passphrase? The token can't be recovered, but re-login mints a
  // fresh one with a new passphrase - the WFM account is never locked out.
  function goToLogin() {
    wfmUnlockDialog?.close();
    open('needs_login', authNext);
  }

  async function performWfmLogin(e: SubmitEvent) {
    e?.preventDefault();
    wfmAuthError = null;
    wfmAuthReportable = false;
    if (wfmLoginPassphrase !== wfmLoginConfirm) {
      wfmAuthError = t('auth.mismatch');
      return;
    }
    wfmAuthBusy = true;
    try {
      if (wfmLoginMode === 'token') {
        await desktopWfmLoginWithToken(wfmLoginToken, wfmLoginPassphrase, wfmLoginPlatform, wfmRemember);
      } else {
        await desktopWfmLogin(wfmLoginPassphrase, wfmLoginPlatform, wfmRemember);
      }
      wfmLoginDialog?.close();
      unlocked();
    } catch (err) {
      const code = err instanceof DesktopCmdError ? err.code : null;
      if (code === 'signin_window_failed') {
        wfmLoginMode = 'token';
        failed(err);
      } else if (code !== 'cancelled') {
        failed(err);
      } else if (wfmLoginDialog?.open) {
        // Closing the WFM window is a choice, not a failure; the Cancel path
        // has already closed this dialog.
        wfmAuthError = t('auth.windowClosed');
      }
    } finally {
      wfmLoginToken = '';
      wfmLoginPassphrase = '';
      wfmLoginConfirm = '';
      wfmAuthBusy = false;
    }
  }

  function cancelWfmLogin() {
    wfmLoginDialog?.close();
    if (wfmAuthBusy) {
      desktopWfmLoginCancel().catch((e) => console.error('closing the sign-in window failed', e));
    }
  }

  async function performWfmUnlock(e: SubmitEvent) {
    e?.preventDefault();
    wfmAuthError = null;
    wfmAuthReportable = false;
    wfmAuthBusy = true;
    try {
      await desktopWfmUnlock(wfmUnlockPassphrase, wfmRemember);
      wfmUnlockDialog?.close();
      unlocked();
    } catch (err) {
      // The login file vanished between the check and the unlock - switch to
      // the login dialog instead of asking for a passphrase that can't work.
      if (err instanceof DesktopCmdError && err.code === 'needs_login') {
        wfmUnlockDialog?.close();
        open('needs_login', authNext);
      } else {
        // bad_passphrase and transient WFM failures stay in the dialog with
        // their message - retry is a re-type away.
        failed(err);
      }
    } finally {
      wfmUnlockPassphrase = '';
      wfmAuthBusy = false;
    }
  }
</script>

<dialog bind:this={wfmLoginDialog} class="cryptobox" data-testid="wfm-login-dialog" oncancel={cancelWfmLogin}>
  <form onsubmit={performWfmLogin}>
    <header>
      <h3>{t('auth.loginTitle')}</h3>
      <p class="muted">
        {t('auth.loginLede')}
      </p>
    </header>
    {#if wfmLoginMode === 'token'}
      <label>
        {t('auth.sessionToken')}
        <input
          type="password"
          autocomplete="off"
          spellcheck="false"
          data-testid="wfm-login-token"
          bind:value={wfmLoginToken}
          required
        />
      </label>
      <WfmTokenGuide />
      <p class="forgot-row muted">
        <button type="button" class="forgot" onclick={() => (wfmLoginMode = 'window')}>{t('auth.useWindow')}</button>
      </p>
    {:else}
      <p class="forgot-row muted">
        {t('auth.windowBroken')}
        <button type="button" class="forgot" data-testid="wfm-login-use-token" onclick={() => (wfmLoginMode = 'token')}>{t('auth.pasteToken')}</button>
        {t('auth.fromBrowser')}
      </p>
    {/if}
    <div class="field">
      <span>{t('auth.platform')}</span>
      <Select label={t('auth.platform')} value={wfmLoginPlatform} options={platformOptions} onchange={(next) => (wfmLoginPlatform = next)} />
    </div>
    <label>
      {t('auth.passphrase')}
      <input
        type="password"
        autocomplete="new-password"
        bind:value={wfmLoginPassphrase}
        bind:this={wfmLoginPassInput}
        placeholder={t('auth.passphraseHint')}
        required
        minlength="12"
      />
    </label>
    <p class="muted">
      {t('auth.passphraseHelp')}
    </p>
    <label>
      {t('auth.confirm')}
      <input type="password" autocomplete="new-password" bind:value={wfmLoginConfirm} required minlength="12"
        aria-describedby="wfm-confirm-hint{wfmAuthError ? ' wfm-login-error' : ''}"
        aria-invalid={wfmAuthError === t('auth.mismatch') ? 'true' : undefined} />
      <span class="muted" id="wfm-confirm-hint">{t('auth.confirmHelp')}</span>
    </label>
    <label class="remember">
      <input type="checkbox" bind:checked={wfmRemember} />
      {t('auth.rememberLong')}
    </label>
    {#if rememberAvailable === false}
      <p class="muted" data-testid="wfm-remember-unavailable">
        {t('auth.noKeyring')}
      </p>
    {/if}
    {#if wfmAuthBusy && wfmLoginMode === 'window'}
      <p class="muted" role="status" data-testid="wfm-login-waiting">
        {t('auth.waiting')}
      </p>
    {/if}
    {#if wfmAuthError}
      <div class="err" id="wfm-login-error" role="alert" data-testid="wfm-auth-error">
        {wfmAuthError}
        {#if wfmAuthReportable && onreport}
          <p class="forgot-row"><button type="button" class="forgot" onclick={onreport}>{t('auth.report')}</button>{t('auth.reportAfter')}</p>
        {/if}
      </div>
    {/if}
    <footer>
      <button type="button" class="ghost" onclick={cancelWfmLogin}>{t('auth.cancel')}</button>
      <button type="submit" disabled={wfmAuthBusy}>
        {#if wfmLoginMode === 'token'}
          {t(wfmAuthBusy ? 'auth.checkingToken' : 'auth.saveToken')}
        {:else}
          {t(wfmAuthBusy ? 'auth.waitingSignIn' : 'auth.continue')}
        {/if}
      </button>
    </footer>
  </form>
</dialog>

<dialog bind:this={wfmUnlockDialog} class="cryptobox" data-testid="wfm-unlock-dialog">
  <form onsubmit={performWfmUnlock}>
    <header>
      <h3>{t('auth.unlockTitle')}</h3>
      <p class="muted">
        {t('auth.unlockLede')}
      </p>
    </header>
    <label>
      {t('auth.passphraseLabel')}
      <input
        type="password"
        autocomplete="current-password"
        data-testid="wfm-unlock-pass"
        aria-describedby={wfmAuthError ? 'wfm-unlock-error' : undefined}
        aria-invalid={wfmAuthError ? 'true' : undefined}
        bind:value={wfmUnlockPassphrase}
        required
        bind:this={wfmUnlockPassInput}
      />
    </label>
    <label class="remember">
      <input type="checkbox" bind:checked={wfmRemember} />
      {t('auth.remember')}
    </label>
    {#if rememberAvailable === false}
      <p class="muted" data-testid="wfm-remember-unavailable">
        {t('auth.noKeyring')}
      </p>
    {/if}
    {#if wfmAuthError}
      <div class="err" id="wfm-unlock-error" role="alert" data-testid="wfm-auth-error">
        {wfmAuthError}
        {#if wfmAuthReportable && onreport}
          <p class="forgot-row"><button type="button" class="forgot" onclick={onreport}>{t('auth.report')}</button>{t('auth.reportAfter')}</p>
        {/if}
      </div>
    {/if}
    <p class="forgot-row muted">
      {t('auth.forgot')} <button type="button" class="forgot" onclick={goToLogin}>{t('auth.loginAgain')}</button>{t('auth.forgotAfter')}
    </p>
    <footer>
      <button type="button" class="ghost" onclick={() => wfmUnlockDialog?.close()}>{t('auth.cancel')}</button>
      <button type="submit" disabled={wfmAuthBusy}>{t(wfmAuthBusy ? 'auth.unlocking' : 'auth.unlock')}</button>
    </footer>
  </form>
</dialog>

<style>
  /* The dialog.cryptobox shell now lives in app.css - this component and
     ExportImportDialogs render the same chrome and had drifted apart. Only
     the remember-me row, which exists in no other dialog, stays here. */
  dialog.cryptobox label.remember {
    flex-direction: row;
    align-items: flex-start;
    gap: 8px;
    text-transform: none;
    letter-spacing: normal;
    font-size: var(--text-caption);
    line-height: 1.45;
  }
  dialog.cryptobox label.remember input { margin-top: 2px; }
  .forgot-row { display: flex; align-items: baseline; gap: 4px; margin: 0; }
  .forgot {
    background: none;
    border: none;
    padding: 0;
    font: inherit;
    font-size: inherit;
    color: var(--accent);
    text-decoration: underline;
    cursor: pointer;
  }
  .forgot:hover { color: var(--fg); }
</style>
