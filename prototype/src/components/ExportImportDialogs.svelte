<script lang="ts">
  // @ts-nocheck — presentation glue (dialog refs, catch blocks, event
  // handlers), same rationale as App.svelte's own @ts-nocheck.
  //
  // Encrypted-export / import dialogs. Triggered imperatively from
  // App.svelte via `bind:this` — openExport() from the toolbar's Export
  // button, openImport(blob) from handleInventory() when a dropped/scanned
  // file turns out to be an encrypted wfminv snapshot rather than a plain
  // inventory.json. Export only reads app state (owned/inventoryName/
  // lastUpdated, passed as props); a successful import reports the decoded
  // snapshot back via `onimport` — App.svelte owns what importing means for
  // its own state (resolved/deltas/market/phase/store.saveSnapshot), the
  // same split as WfmAuthDialogs' onunlocked.
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
  let exportPass = $state('');
  let exportConfirm = $state('');
  let exportBusy = $state(false);
  let importPass = $state('');
  let importBlob = $state(null);
  let importBusy = $state(false);
  let cryptoError = $state(null);

  export function openExport() {
    cryptoError = null;
    exportPass = '';
    exportConfirm = '';
    exportDialog?.showModal();
  }

  export function openImport(blob) {
    cryptoError = null;
    importPass = '';
    importBlob = blob;
    importDialog?.showModal();
  }

  async function performExport(e) {
    e?.preventDefault();
    cryptoError = null;
    if (exportPass !== exportConfirm) {
      cryptoError = "Passphrases don't match.";
      return;
    }
    if (exportPass.length < 4) {
      cryptoError = 'Passphrase must be at least 4 characters.';
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

  async function performImport(e) {
    e?.preventDefault();
    cryptoError = null;
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
      importDialog?.close();
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
        another device with the same passphrase. Nothing leaves your browser.
      </p>
    </header>
    <label>
      Passphrase
      <input
        type="password"
        autocomplete="new-password"
        bind:value={exportPass}
        placeholder="something only you'd type"
        required
        minlength="4"
        autofocus
      />
    </label>
    <label>
      Confirm
      <input
        type="password"
        autocomplete="new-password"
        bind:value={exportConfirm}
        required
        minlength="4"
      />
    </label>
    {#if cryptoError}
      <div class="err">{cryptoError}</div>
    {/if}
    <footer>
      <button type="button" class="ghost" onclick={() => exportDialog?.close()}>Cancel</button>
      <button type="submit" disabled={exportBusy}>{exportBusy ? 'Encrypting…' : 'Download'}</button>
    </footer>
  </form>
</dialog>

<dialog bind:this={importDialog} class="cryptobox">
  <form onsubmit={performImport}>
    <header>
      <h3>Decrypt snapshot</h3>
      <p class="muted">
        This looks like an encrypted wfminv snapshot. Enter the passphrase you
        used when exporting it.
      </p>
    </header>
    <label>
      Passphrase
      <input
        type="password"
        autocomplete="current-password"
        bind:value={importPass}
        required
        minlength="4"
        autofocus
      />
    </label>
    {#if cryptoError}
      <div class="err">{cryptoError}</div>
    {/if}
    <footer>
      <button type="button" class="ghost" onclick={() => importDialog?.close()}>Cancel</button>
      <button type="submit" disabled={importBusy}>{importBusy ? 'Decrypting…' : 'Decrypt'}</button>
    </footer>
  </form>
</dialog>

<style>
  /* Duplicated from App.svelte's shared .cryptobox dialog styling — same
     rationale as WfmAuthDialogs.svelte's copy (Svelte scopes CSS
     per-component). Only the password-input subset — these dialogs have no
     email/select/remember-checkbox fields. */
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
  dialog.cryptobox input[type="password"] {
    font: inherit;
    color: var(--fg);
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 8px 10px;
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  }
  dialog.cryptobox input[type="password"]:focus {
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
