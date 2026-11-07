<script lang="ts">
  import { t } from '../../i18n/i18n.svelte';
  import Rich from '../../ui/Rich.svelte';
  import { useDesktopServices } from '../shell/desktop-context';
  const { encryptPayload, decryptPayload } = useDesktopServices();
  import { humanError } from '../../contracts/errors';
  import type { OwnedRecord } from '../../contracts/data';
  import { hasMinimumPassphraseLength, type EncryptedBlob } from '../../contracts/encrypted-snapshot';
  import { tick } from 'svelte';
  
  import { buildSnapshotPayload } from '../../domain/snapshot';

  let { owned, inventoryName, lastUpdated, onimport }: {
    owned: Map<string, OwnedRecord>;
    inventoryName: string | null;
    lastUpdated: number | null;
    onimport: (result: { invName: string; ts: number; ownedMap: Map<string, OwnedRecord> }) => Promise<void> | void;
  } = $props();

  let exportDialog = $state<HTMLDialogElement>();
  let importDialog = $state<HTMLDialogElement>();
  let exportPassInput = $state<HTMLInputElement>();
  let importPassInput = $state<HTMLInputElement>();
  let exportPass = $state('');
  let exportConfirm = $state('');
  let exportBusy = $state(false);
  let importPass = $state('');
  let importBlob = $state<EncryptedBlob | null>(null);
  let importName = $state('');
  let importBusy = $state(false);
  let importConfirming = $state(false);
  let restoreWarning = $state<HTMLElement>();
  let cryptoError = $state<string | null>(null);

  $effect(() => {
    if (importConfirming) {
      // The passphrase field leaves the DOM at this point. Move focus to the
      // warning so keyboard and screen-reader users do not lose their place.
      void tick().then(() => restoreWarning?.focus());
    }
  });

  export function openExport() {
    cryptoError = null;
    exportPass = '';
    exportConfirm = '';
    exportDialog?.showModal();
    exportPassInput?.focus();
  }

  let importFileInput = $state<HTMLInputElement>();
  export function pickImport() {
    cryptoError = null;
    importFileInput?.click();
  }
  async function onImportPicked(e: Event) {
    const file = (e.currentTarget as HTMLInputElement).files?.[0];
    (e.currentTarget as HTMLInputElement).value = '';
    if (!file) return;
    importConfirming = false;
    importBlob = null;
    importName = '';
    importPass = '';
    cryptoError = null;
    try {
      const text = await file.text();
      const blob = JSON.parse(text);
      // Real snapshots carry the format marker; anything else is not ours
      // (the real gate is decryptPayload's isEncrypted, this is just a
      // friendlier pre-screen than failing at the passphrase step).
      if (!blob || blob.format !== 'wfminv-encrypted-v1') {
        throw new Error("That doesn't look like an encrypted wfminv snapshot.");
      }
      importBlob = blob;
      importName = file.name;
      importDialog?.showModal();
      importPassInput?.focus();
    } catch (err) {
      cryptoError = humanError(err);
      importDialog?.showModal();
      importPassInput?.focus();
    }
  }

  async function performExport(e: SubmitEvent) {
    e?.preventDefault();
    cryptoError = null;
    if (exportPass !== exportConfirm) {
      cryptoError = t('backup.mismatch');
      return;
    }
    if (!hasMinimumPassphraseLength(exportPass)) {
      cryptoError = t('backup.tooShort');
      return;
    }
    exportBusy = true;
    try {
      // Same builder the stores use, so an export can never drift from what
      // gets persisted. Timestamp is the snapshot's own, not now().
      const payload = buildSnapshotPayload({ invName: inventoryName ?? 'inventory', owned }, lastUpdated ?? Date.now());
      const blob = await encryptPayload(payload, exportPass);
      const text = JSON.stringify(blob);
      const file = new Blob([text], { type: 'application/json' });
      const url = URL.createObjectURL(file);
      const a = document.createElement('a');
      const stamp = new Date().toISOString().slice(0, 10);
      a.href = url;
      a.download = `wfminv-${stamp}.json`;
      document.body.appendChild(a);
      a.click();
      a.remove();
      URL.revokeObjectURL(url);
      exportDialog?.close();
    } catch (err) {
      cryptoError = humanError(err);
    } finally {
      exportBusy = false;
    }
  }

  function resetImport() {
    importConfirming = false;
    importBlob = null;
    importName = '';
    importPass = '';
    cryptoError = null;
  }

  function closeImport() {
    resetImport();
    importDialog?.close();
  }

  function editImport() {
    importConfirming = false;
    void tick().then(() => importPassInput?.focus());
  }

  function requestImport(e: SubmitEvent) {
    e?.preventDefault();
    cryptoError = null;
    if ((owned?.size ?? 0) > 0) {
      // Do not decrypt until the destructive consequence has been confirmed
      // inside the app. Keeping this as a distinct state also prevents the
      // selected file or passphrase changing underneath the confirmation.
      importConfirming = true;
      return;
    }
    performImport();
  }

  async function performImport() {
    cryptoError = null;
    importConfirming = false;
    importBusy = true;
    try {
      if (!importBlob) throw new Error(t('backup.chooseFirst'));
      const payload = await decryptPayload(importBlob, importPass) as { owned?: Array<[string, OwnedRecord]>; invName?: string; ts?: number } | null;
      if (!Array.isArray(payload?.owned)) {
        throw new Error(t('backup.missingItems'));
      }
      // Old (pre-subtype) exports stored the slug as the map key and lacked
      // rec.slug / rec.subtype - backfill from the key so they still load.
      const ownedMap = new Map<string, OwnedRecord>(
        payload.owned.map(([key, rec]) => [
          key.includes('|') ? key : `${key}|`,
          {
            ...rec,
            slug: rec.slug ?? (key.includes('|') ? key.split('|')[0] : key),
            subtype: rec.subtype ?? null,
            // Older exports predate the leveled-gear feature - default to 0
            // (unknown) rather than leaving it undefined, which sellableQty's
            // default param would also catch but keeps the record shape honest.
            leveled: rec.leveled ?? 0,
          },
        ])
      );
      await onimport({ invName: payload.invName || 'imported.json', ts: payload.ts || Date.now(), ownedMap });
      closeImport();
    } catch (err) {
      cryptoError = humanError(err);
    } finally {
      importBusy = false;
    }
  }
</script>

<dialog bind:this={exportDialog} class="cryptobox">
  <form onsubmit={performExport}>
    <header>
      <h3>{t('backup.exportTitle')}</h3>
      <p class="muted">{t('backup.exportIntro')}</p>
    </header>
    <label>
      {t('backup.passphrase')}
      <input
        type="password"
        autocomplete="new-password"
        bind:value={exportPass}
        placeholder={t('backup.passphraseHint')}
        required
        minlength="12"
        bind:this={exportPassInput}
      />
    </label>
    <label>
      {t('backup.confirm')}
      <input
        type="password"
        autocomplete="new-password"
        bind:value={exportConfirm}
        required
        minlength="12"
      />
    </label>
    {#if cryptoError}
      <div class="err" role="alert">{cryptoError}</div>
    {/if}
    <footer>
      <button type="button" class="ghost" onclick={() => exportDialog?.close()}>{t('backup.cancel')}</button>
      <button type="submit" disabled={exportBusy}>{t(exportBusy ? 'backup.encrypting' : 'backup.download')}</button>
    </footer>
  </form>
</dialog>

<dialog bind:this={importDialog} class="cryptobox" onclose={resetImport}>
  <form onsubmit={requestImport}>
    <header>
      <h3>{t('backup.restoreTitle')}</h3>
      {#if importConfirming}
        <p class="muted">{t('backup.reviewFirst')}</p>
      {:else}
        <p class="muted"><Rich text={t('backup.restoreIntro')} /></p>
      {/if}
    </header>
    {#if importConfirming}
      <section
        class="restore-warning"
        aria-labelledby="restore-warning-title"
        tabindex="-1"
        bind:this={restoreWarning}
      >
        <strong id="restore-warning-title">{t('backup.replaceTitle')}</strong>
        <p>{t('backup.replaceBody', { file: importName, count: owned?.size ?? 0 })}</p>
        <p>{t('backup.noUndo')}</p>
      </section>
      <footer>
        <button type="button" class="ghost" onclick={editImport}>{t('backup.back')}</button>
        <button type="button" class="danger" disabled={importBusy} onclick={performImport}>
          {t(importBusy ? 'backup.decrypting' : 'backup.confirmRestore')}
        </button>
      </footer>
    {:else}
      <label>
        {t('backup.file')}
        <button type="button" class="ghost" onclick={pickImport}>{t('backup.chooseFile')}</button>
        {#if importBlob}
          <span class="muted small file-name">{t('backup.selected', { file: importName })}</span>
        {/if}
      </label>
      <input
        bind:this={importFileInput}
        type="file"
        accept="application/json,.json"
        onchange={onImportPicked}
        style="display:none"
      />
      <label>
        {t('backup.passphrase')}
        <input
          type="password"
          autocomplete="current-password"
          bind:value={importPass}
          placeholder={t('backup.passphrasePlaceholder')}
          required
          bind:this={importPassInput}
        />
      </label>
      {#if cryptoError}
        <div class="err" role="alert">{cryptoError}</div>
      {/if}
      <footer>
        <button type="button" class="ghost" onclick={closeImport}>{t('backup.cancel')}</button>
        <button type="submit" disabled={importBusy || !importBlob}>
          {t(importBusy ? 'backup.decrypting' : (owned?.size ?? 0) > 0 ? 'backup.reviewRestore' : 'backup.decrypt')}
        </button>
      </footer>
    {/if}
  </form>
</dialog>

<style>
  /* Duplicated from App.svelte's shared .cryptobox dialog styling - same
     rationale as WfmAuthDialogs.svelte's copy (Svelte scopes CSS
     per-component). Only the password-input subset - these dialogs have no
     email/select/remember-checkbox fields. */
  label { gap: 8px; }
  label .ghost { width: max-content; }
  .file-name { font-size: var(--text-caption); }
  .restore-warning {
    color: var(--fg);
    background: color-mix(in srgb, var(--bad) 10%, transparent);
    border: 1px solid color-mix(in srgb, var(--bad) 40%, var(--border));
    border-radius: var(--radius-ctl);
    padding: 12px;
  }
  .restore-warning strong { color: var(--bad); }
  .restore-warning p { margin: 8px 0 0; font-size: var(--text-control); line-height: 1.5; }
  button.danger { color: var(--bad); border-color: var(--bad); }
</style>
