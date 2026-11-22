<script lang="ts">
  import { useDesktopServices } from '../shell/desktop-context';
  const { desktopWfmLogin, desktopWfmLoginCancel, desktopWfmLoginWithToken, desktopWfmUnlock } = useDesktopServices();
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
  // Shown only when the saved sign-in could not be opened; a working one opens
  // with nothing on screen.
  let wfmUnlockDialog = $state<HTMLDialogElement>();
  let wfmLoginPlatform = $state('pc');
  // 'token' is the fallback for when the sign-in window cannot work (its page
  // crashes on Linux when GStreamer lacks an audio sink): the user signs in with their
  // own browser and pastes its JWT cookie.
  let wfmLoginMode = $state<'window' | 'token'>('window');
  let wfmLoginSubmit = $state<HTMLButtonElement>();
  let wfmLoginToken = $state('');
  let wfmAuthBusy = $state(false);
  let wfmAuthError = $state<string | null>(null);
  // A closed window or a keyring that did not answer is the user's to fix, not a bug.
  let wfmAuthReportable = $state(false);
  // What to do once the session unlocks: 'list' re-opens the listing flow the
  // CTA started; null (the Resume path) leaves the caller where it was.
  // Threaded through to `onunlocked` rather than acted on here - App.svelte
  // owns what 'list' means (opening its own listing state).
  let authNext: string | null = null;

  function failed(err: unknown) {
    wfmAuthError = humanError(err);
    wfmAuthReportable = !(err instanceof DesktopCmdError && err.code === 'keyring_unavailable');
    if (wfmAuthReportable) onautherror?.(err);
  }

  export async function open(code: string, next: string | null = null) {
    wfmAuthError = null;
    wfmAuthReportable = false;
    authNext = next;
    if (code === 'needs_login') {
      wfmLoginDialog?.showModal();
      // The dialog would focus its first control, the paste-a-token link: one
      // more press of A on a controller would then ask for a keyboard.
      if (wfmLoginMode === 'window') wfmLoginSubmit?.focus();
    } else {
      await performWfmUnlock();
    }
  }

  function unlocked() {
    onautherror?.(null);
    onunlocked(authNext);
    authNext = null;
  }

  function goToLogin() {
    wfmUnlockDialog?.close();
    open('needs_login', authNext);
  }

  async function performWfmLogin(e: SubmitEvent) {
    e?.preventDefault();
    wfmAuthError = null;
    wfmAuthReportable = false;
    wfmAuthBusy = true;
    try {
      if (wfmLoginMode === 'token') {
        await desktopWfmLoginWithToken(wfmLoginToken, wfmLoginPlatform);
      } else {
        await desktopWfmLogin(wfmLoginPlatform);
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
      wfmAuthBusy = false;
    }
  }

  function cancelWfmLogin() {
    wfmLoginDialog?.close();
    if (wfmAuthBusy) {
      desktopWfmLoginCancel().catch((e) => console.error('closing the sign-in window failed', e));
    }
  }

  // Opens the saved sign-in with the key this device kept. Only a failure puts
  // anything on screen: a sign-in that can no longer open goes straight to the
  // sign-in dialog, anything else (a keyring that did not answer, a network
  // failure) to a dialog that can retry.
  async function performWfmUnlock(e?: SubmitEvent) {
    e?.preventDefault();
    wfmAuthError = null;
    wfmAuthReportable = false;
    wfmAuthBusy = true;
    try {
      await desktopWfmUnlock();
      wfmUnlockDialog?.close();
      unlocked();
    } catch (err) {
      if (err instanceof DesktopCmdError && err.code === 'needs_login') {
        wfmUnlockDialog?.close();
        open('needs_login', authNext);
      } else {
        failed(err);
        if (!wfmUnlockDialog?.open) wfmUnlockDialog?.showModal();
      }
    } finally {
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
      <button type="submit" disabled={wfmAuthBusy} bind:this={wfmLoginSubmit}>
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
    {#if wfmAuthError}
      <div class="err" id="wfm-unlock-error" role="alert" data-testid="wfm-auth-error">
        {wfmAuthError}
        {#if wfmAuthReportable && onreport}
          <p class="forgot-row"><button type="button" class="forgot" onclick={onreport}>{t('auth.report')}</button>{t('auth.reportAfter')}</p>
        {/if}
      </div>
    {/if}
    <footer>
      <button type="button" class="ghost" onclick={() => wfmUnlockDialog?.close()}>{t('auth.cancel')}</button>
      <button type="button" class="ghost" onclick={goToLogin}>{t('auth.loginAgain')}</button>
      <button type="submit" disabled={wfmAuthBusy}>{t(wfmAuthBusy ? 'auth.unlocking' : 'auth.retry')}</button>
    </footer>
  </form>
</dialog>

<style>
  /* The dialog.cryptobox shell lives in app.css, shared with
     ExportImportDialogs. */
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
