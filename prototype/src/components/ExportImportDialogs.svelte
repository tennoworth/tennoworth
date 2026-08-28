<script lang="ts">
  // @ts-nocheck — presentation glue (dialog refs, catch blocks, event
  // handlers), same rationale as App.svelte's own @ts-nocheck.
  //
  // Encrypted-export / import dialogs. Triggered imperatively from
  // App.svelte via `bind:this` — openExport() from the toolbar's Export
  // button, and the import path has its own file picker in this component
  // (the old DropZone flow that routed an encrypted drop to openImport() is
  // gone — the app scans from the game and takes no files). Export only reads
  // app state (owned/inventoryName/lastUpdated, passed as props); a successful
  // import reports the decoded snapshot back via `onimport` — App.svelte owns
  // what importing means for its own state (resolved/deltas/market/phase/
  // store.saveSnapshot), the same split as WfmAuthDialogs' onunlocked.
  import { tick } from 'svelte';
  import { encryptPayload, decryptPayload } from '../lib/crypto';
  import { buildSnapshotPayload } from '../lib/storage';

  let { owned, inventoryName, lastUpdated, onimport }: {
    owned: Map<string, any>;
    inventoryName: string | null;
    lastUpdated: number | null;
    onimport: (result: { invName: string; ts: number; ownedMap: Map<string, any> }) => Promise<void> | void;
  } = $props();

  let exportDialog = $state();
  let importDialog = $state();
  let exportPassInput = $state<HTMLInputElement>();
  let importPassInput = $state<HTMLInputElement>();
  let exportPass = $state('');
  let exportConfirm = $state('');
  let exportBusy = $state(false);
  let importPass = $state('');
  let importBlob = $state(null);
  let importName = $state('');
  let importBusy = $state(false);
  let importConfirming = $state(false);
  let restoreWarning = $state<HTMLElement>();
  let cryptoError = $state(null);

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

  let importFileInput = $state();
  export function pickImport() {
    cryptoError = null;
    importFileInput?.click();
  }
  async function onImportPicked(e) {
    const file = e.target.files?.[0];
    e.target.value = '';
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
      cryptoError = err.message || String(err);
      importDialog?.showModal();
      importPassInput?.focus();
    }
  }

  async function performExport(e) {
    e?.preventDefault();
    cryptoError = null;
    if (exportPass !== exportConfirm) {
      cryptoError = "Passphrases don't match.";
      return;
    }
    if (exportPass.length < 12) {
      cryptoError = 'Passphrase must be at least 12 characters — same floor as your login passphrase.';
      return;
    }
    exportBusy = true;
    try {
      // Same builder the stores use, so an export can never drift from what
      // gets persisted. Timestamp is the snapshot's own, not now().
      const payload = buildSnapshotPayload({ invName: inventoryName, owned }, lastUpdated);
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
      cryptoError = err.message || String(err);
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

  function requestImport(e) {
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
      const payload = await decryptPayload(importBlob, importPass);
      if (!Array.isArray(payload?.owned)) {
        throw new Error('Decrypted file is missing the owned-items array.');
      }
      // Old (pre-subtype) exports stored the slug as the map key and lacked
      // rec.slug / rec.subtype — backfill from the key so they still load.
      const ownedMap = new Map(
        payload.owned.map(([key, rec]) => [
          key.includes('|') ? key : `${key}|`,
          {
            ...rec,
            slug: rec.slug ?? (key.includes('|') ? key.split('|')[0] : key),
            subtype: rec.subtype ?? null,
            // Older exports predate the leveled-gear feature — default to 0
            // (unknown) rather than leaving it undefined, which sellableQty's
            // default param would also catch but keeps the record shape honest.
            leveled: rec.leveled ?? 0,
          },
        ])
      );
      await onimport({ invName: payload.invName || 'imported.json', ts: payload.ts || Date.now(), ownedMap });
      closeImport();
    } catch (err) {
      cryptoError = err.message || String(err);
    } finally {
      importBusy = false;
    }
  }
</script>

<dialog bind:this={exportDialog} class="cryptobox">
  <form onsubmit={performExport}>
    <header>
      <h3>Export encrypted snapshot</h3>
      <p class="muted">
        Saves your resolved inventory as an encrypted JSON file. Decrypt on
        another device with the same passphrase. Nothing leaves this app.
      </p>
    </header>
    <label>
      Passphrase
      <input
        type="password"
        autocomplete="new-password"
        bind:value={exportPass}
        placeholder="12+ characters — same floor as your login passphrase"
        required
        minlength="12"
        bind:this={exportPassInput}
      />
    </label>
    <label>
      Confirm
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
      <button type="button" class="ghost" onclick={() => exportDialog?.close()}>Cancel</button>
      <button type="submit" disabled={exportBusy}>{exportBusy ? 'Encrypting…' : 'Download'}</button>
    </footer>
  </form>
</dialog>

<dialog bind:this={importDialog} class="cryptobox" onclose={resetImport}>
  <form onsubmit={requestImport}>
    <header>
      <h3>Restore encrypted snapshot</h3>
      {#if importConfirming}
        <p class="muted">Review the restore before changing your inventory.</p>
      {:else}
        <p class="muted">
          Pick a <code>wfminv-*.json</code> backup exported from another device,
          then enter the passphrase you used when exporting it.
        </p>
      {/if}
    </header>
    {#if importConfirming}
      <section
        class="restore-warning"
        aria-labelledby="restore-warning-title"
        tabindex="-1"
        bind:this={restoreWarning}
      >
        <strong id="restore-warning-title">Replace current inventory?</strong>
        <p>
          Restoring <span class="file-name">{importName}</span> will replace your
          current {owned?.size ?? 0}-item inventory after the backup is decrypted
          and validated successfully.
        </p>
        <p>This cannot be undone unless you have another exported backup.</p>
      </section>
      <footer>
        <button type="button" class="ghost" onclick={editImport}>Back</button>
        <button type="button" class="danger" disabled={importBusy} onclick={performImport}>
          {importBusy ? 'Decrypting…' : 'Confirm restore'}
        </button>
      </footer>
    {:else}
      <label>
        Backup file
        <button type="button" class="ghost" onclick={pickImport}>Choose file…</button>
        {#if importBlob}
          <span class="muted small file-name">{importName} — selected, ready to decrypt</span>
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
        Passphrase
        <input
          type="password"
          autocomplete="current-password"
          bind:value={importPass}
          placeholder="Type the passphrase this file was exported with."
          required
          bind:this={importPassInput}
        />
      </label>
      {#if cryptoError}
        <div class="err" role="alert">{cryptoError}</div>
      {/if}
      <footer>
        <button type="button" class="ghost" onclick={closeImport}>Cancel</button>
        <button type="submit" disabled={importBusy || !importBlob}>
          {importBusy ? 'Decrypting…' : (owned?.size ?? 0) > 0 ? 'Review restore' : 'Decrypt'}
        </button>
      </footer>
    {/if}
  </form>
</dialog>

<style>
  /* Duplicated from App.svelte's shared .cryptobox dialog styling — same
     rationale as WfmAuthDialogs.svelte's copy (Svelte scopes CSS
     per-component). Only the password-input subset — these dialogs have no
     email/select/remember-checkbox fields. */
  label { gap: 8px; }
  label .ghost { width: max-content; }
  .file-name { font-size: 12px; }
  .restore-warning {
    color: var(--fg);
    background: color-mix(in srgb, var(--bad) 10%, transparent);
    border: 1px solid color-mix(in srgb, var(--bad) 40%, var(--border));
    border-radius: var(--radius-ctl);
    padding: 12px;
  }
  .restore-warning strong { color: var(--bad); }
  .restore-warning p { margin: 8px 0 0; font-size: 12.5px; line-height: 1.5; }
  button.danger { color: var(--bad); border-color: var(--bad); }
</style>
