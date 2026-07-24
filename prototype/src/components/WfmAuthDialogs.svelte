<script lang="ts">
  // @ts-nocheck — presentation glue (dialog refs, catch blocks, event
  // handlers), same rationale as App.svelte's own @ts-nocheck: the
  // high-value typing lives at the lib/ boundary this component calls into.
  // Desktop WFM login + unlock dialogs. Triggered from App.svelte (the Sell
  // CTA's proactive status check, the pending-plan Resume flow's needs_login/
  // needs_unlock rejections, and ListingReviewModal's onauthrequired) via
  // `bind:this` + the exported `open()` — the trigger sites live in three
  // different places in App.svelte, so this stays an imperative API rather
  // than a prop the parent sets. Secrets go straight into the wfm_login /
  // unlock_jwt commands and the bound fields are cleared as soon as the call
  // returns — nothing lingers in webview state.
  import {
    desktopWfmLogin, desktopWfmUnlock, desktopTrySilentUnlock, DesktopCmdError,
  } from '../lib/transport';

  let { onunlocked }: { onunlocked: (next: string | null) => void } = $props();

  let wfmLoginDialog = $state();
  let wfmUnlockDialog = $state();
  let wfmLoginEmail = $state('');
  let wfmLoginPassword = $state('');
  let wfmLoginPassphrase = $state('');
  let wfmLoginConfirm = $state('');
  let wfmLoginPlatform = $state('pc');
  let wfmUnlockPassphrase = $state('');
  // "Remember on this device" (OS keyring). One preference shared by the
  // login and unlock dialogs; default on — the browser-cookie parity call.
  let wfmRemember = $state(true);
  let wfmAuthBusy = $state(false);
  let wfmAuthError = $state(null);
  // What to do once the session unlocks: 'list' re-opens the listing flow the
  // CTA started; null (the Resume path) leaves the caller where it was.
  // Threaded through to `onunlocked` rather than acted on here — App.svelte
  // owns what 'list' means (opening its own listingOpen state).
  let authNext: string | null = null;

  export async function open(code: string, next: string | null = null) {
    wfmAuthError = null;
    authNext = next;
    if (code === 'needs_login') {
      wfmLoginPassword = '';
      wfmLoginPassphrase = '';
      wfmLoginConfirm = '';
      wfmLoginDialog?.showModal();
    } else {
      // A remembered device key (OS keyring) unlocks without the modal. Any
      // miss — no entry, no keyring daemon, stale key — falls through to the
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
    }
  }

  function unlocked() {
    onunlocked(authNext);
    authNext = null;
  }

  async function performWfmLogin(e) {
    e?.preventDefault();
    wfmAuthError = null;
    if (wfmLoginPassphrase !== wfmLoginConfirm) {
      wfmAuthError = "Passphrases don't match.";
      return;
    }
    wfmAuthBusy = true;
    try {
      await desktopWfmLogin(wfmLoginEmail.trim(), wfmLoginPassword, wfmLoginPassphrase, wfmLoginPlatform, wfmRemember);
      wfmLoginDialog?.close();
      unlocked();
    } catch (err) {
      wfmAuthError = err.message || String(err);
    } finally {
      wfmLoginPassword = '';
      wfmLoginPassphrase = '';
      wfmLoginConfirm = '';
      wfmAuthBusy = false;
    }
  }

  async function performWfmUnlock(e) {
    e?.preventDefault();
    wfmAuthError = null;
    wfmAuthBusy = true;
    try {
      await desktopWfmUnlock(wfmUnlockPassphrase, wfmRemember);
      wfmUnlockDialog?.close();
      unlocked();
    } catch (err) {
      // The login file vanished between the check and the unlock — switch to
      // the login dialog instead of asking for a passphrase that can't work.
      if (err instanceof DesktopCmdError && err.code === 'needs_login') {
        wfmUnlockDialog?.close();
        open('needs_login', authNext);
      } else {
        // bad_passphrase and transient WFM failures stay in the dialog with
        // their message — retry is a re-type away.
        wfmAuthError = err.message || String(err);
      }
    } finally {
      wfmUnlockPassphrase = '';
      wfmAuthBusy = false;
    }
  }
</script>

<dialog bind:this={wfmLoginDialog} class="cryptobox" data-testid="wfm-login-dialog">
  <form onsubmit={performWfmLogin}>
    <header>
      <h3>Log in to warframe.market</h3>
      <p class="muted">
        Listing needs your WFM account once. The sign-in token is encrypted
        with your passphrase and stored only on this machine; your password is
        used for this sign-in and never stored.
      </p>
    </header>
    <label>
      Email
      <input type="email" autocomplete="username" bind:value={wfmLoginEmail} required autofocus />
    </label>
    <label>
      Password
      <input type="password" autocomplete="current-password" bind:value={wfmLoginPassword} required />
    </label>
    <label>
      Platform
      <select bind:value={wfmLoginPlatform}>
        <option value="pc">PC (Steam &amp; Epic)</option>
        <option value="ps4">PlayStation</option>
        <option value="xbox">Xbox</option>
        <option value="switch">Switch</option>
      </select>
    </label>
    <label>
      Encryption passphrase
      <input
        type="password"
        autocomplete="new-password"
        bind:value={wfmLoginPassphrase}
        placeholder="min 12 characters — guards the stored token"
        required
        minlength="12"
      />
    </label>
    <label>
      Confirm passphrase
      <input type="password" autocomplete="new-password" bind:value={wfmLoginConfirm} required minlength="12" />
    </label>
    <label class="remember">
      <input type="checkbox" bind:checked={wfmRemember} />
      Remember on this device — stores the unlock key in your OS keyring
      (KWallet, GNOME Keyring, Windows Credential Manager) so you're not asked
      each launch. Never the passphrase itself.
    </label>
    {#if wfmAuthError}
      <div class="err" data-testid="wfm-auth-error">{wfmAuthError}</div>
    {/if}
    <footer>
      <button type="button" class="ghost" onclick={() => wfmLoginDialog?.close()}>Cancel</button>
      <button type="submit" disabled={wfmAuthBusy}>{wfmAuthBusy ? 'Signing in…' : 'Log in'}</button>
    </footer>
  </form>
</dialog>

<dialog bind:this={wfmUnlockDialog} class="cryptobox" data-testid="wfm-unlock-dialog">
  <form onsubmit={performWfmUnlock}>
    <header>
      <h3>Unlock warframe.market listing</h3>
      <p class="muted">
        Enter the passphrase you set at login to decrypt your WFM token for
        this session. It stays in the app's memory — never on disk, never in
        this window.
      </p>
    </header>
    <label>
      Passphrase
      <input
        type="password"
        autocomplete="current-password"
        data-testid="wfm-unlock-pass"
        bind:value={wfmUnlockPassphrase}
        required
        autofocus
      />
    </label>
    <label class="remember">
      <input type="checkbox" bind:checked={wfmRemember} />
      Remember on this device — stores the unlock key in your OS keyring so
      you're not asked each launch. Never the passphrase itself.
    </label>
    {#if wfmAuthError}
      <div class="err" data-testid="wfm-auth-error">{wfmAuthError}</div>
    {/if}
    <footer>
      <button type="button" class="ghost" onclick={() => wfmUnlockDialog?.close()}>Cancel</button>
      <button type="submit" disabled={wfmAuthBusy}>{wfmAuthBusy ? 'Unlocking…' : 'Unlock'}</button>
    </footer>
  </form>
</dialog>

<style>
  /* Duplicated from App.svelte's shared .cryptobox dialog styling — Svelte
     scopes CSS per-component, and this codebase's existing extracted
     components already re-declare shared visual classes rather than
     promoting them to global CSS (see DesktopUpdateBanner.svelte). The
     export/import dialogs (still in App.svelte) carry their own copy too. */
  dialog.cryptobox {
    background: var(--panel);
    color: var(--fg);
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 0;
    max-width: 420px;
    width: calc(100% - 32px);
  }
  dialog.cryptobox::backdrop {
    background: rgba(0, 0, 0, 0.55);
    backdrop-filter: blur(2px);
  }
  dialog.cryptobox form {
    display: flex;
    flex-direction: column;
    gap: 14px;
    padding: 20px 20px 16px;
  }
  dialog.cryptobox header { display: flex; flex-direction: column; gap: 6px; }
  dialog.cryptobox h3 {
    margin: 0;
    font-size: 13px;
    letter-spacing: 0.05em;
    text-transform: uppercase;
    color: var(--accent);
    font-weight: 600;
  }
  dialog.cryptobox header p { margin: 0; font-size: 12.5px; line-height: 1.5; }
  dialog.cryptobox label {
    display: flex;
    flex-direction: column;
    gap: 4px;
    font-size: 11.5px;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: var(--muted);
  }
  dialog.cryptobox label.remember {
    flex-direction: row;
    align-items: flex-start;
    gap: 8px;
    text-transform: none;
    letter-spacing: normal;
    font-size: 12px;
    line-height: 1.45;
  }
  dialog.cryptobox label.remember input { margin-top: 2px; }
  dialog.cryptobox input[type="password"],
  dialog.cryptobox input[type="email"],
  dialog.cryptobox select {
    font: inherit;
    color: var(--fg);
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 8px 10px;
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  }
  dialog.cryptobox input[type="password"]:focus,
  dialog.cryptobox input[type="email"]:focus,
  dialog.cryptobox select:focus {
    border-color: var(--accent);
  }
  dialog.cryptobox .err {
    color: var(--bad);
    font-size: 12px;
    background: color-mix(in srgb, var(--bad) 12%, transparent);
    border: 1px solid color-mix(in srgb, var(--bad) 40%, var(--border));
    padding: 8px 10px;
    border-radius: 6px;
  }
  dialog.cryptobox footer {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    padding-top: 4px;
    border: none;
  }
</style>
