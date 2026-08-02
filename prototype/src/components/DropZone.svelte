<script lang="ts">
  import { humanError } from '../lib/errors';

  interface Props {
    loading?: boolean;
    /** Desktop webview: the scan button is the primary path, so the hint
     *  changes to match. The hosted informational site has no scan. */
    isDesktop?: boolean;
    oninventory?: (event: { name: string; data: unknown }) => void;
  }
  let { loading = false, isDesktop = false, oninventory }: Props = $props();

  let dragOver = $state(false);
  let parseError = $state<string | null>(null);
  let fileInput: HTMLInputElement | undefined = $state();

  async function handleFile(file: File | undefined): Promise<void> {
    parseError = null;
    if (!file) return;
    try {
      const text = await file.text();
      const data = JSON.parse(text);
      oninventory?.({ name: file.name, data });
    } catch (e) {
      parseError = `Couldn't parse ${file.name} as JSON: ${humanError(e)}`;
    }
  }

  function onDragOver(e: DragEvent): void {
    e.preventDefault();
    dragOver = true;
  }

  function onDrop(e: DragEvent): void {
    e.preventDefault();
    dragOver = false;
    const file = e.dataTransfer?.files?.[0];
    handleFile(file);
  }

  function onPicked(e: Event): void {
    const file = (e.target as HTMLInputElement).files?.[0];
    handleFile(file);
  }
</script>

<div
  class="dropzone"
  class:over={dragOver}
  ondragover={onDragOver}
  ondragleave={() => (dragOver = false)}
  ondrop={onDrop}
  role="button"
  tabindex="0"
  onclick={() => fileInput?.click()}
  onkeydown={(e) => (e.key === 'Enter' ? fileInput?.click() : null)}
>
  {#if loading}
    <strong>Loading item catalogs…</strong>
    <p>(one-time per day — the item catalog is ~2 MB)</p>
  {:else}
    <strong>Drop your <code>inventory.json</code> here</strong>
    <p>or click to pick a file</p>
    {#if isDesktop}
      <p class="hint">
        Don't have one? Use <strong>Scan inventory</strong> above — the app
        reads the running game directly. Exports from another tool (AlecaFrame,
        Sainan's <code>warframe-api-helper</code>) drop in here too.
      </p>
    {:else}
      <p class="hint">
        This site is informational — it shows market trends, not your account.
        To get an <code>inventory.json</code>, use the
        <a href="https://github.com/tennoworth/tennoworth/releases" target="_blank" rel="noopener noreferrer">desktop app</a>.
        Exports from another tool (AlecaFrame, Sainan's
        <code>warframe-api-helper</code>) drop in here too.
      </p>
    {/if}
  {/if}
  <input
    bind:this={fileInput}
    type="file"
    accept="application/json,.json"
    onchange={onPicked}
    style="display:none"
  />
  {#if parseError}
    <div class="error">{parseError}</div>
  {/if}
</div>

<style>
  .dropzone {
    border: 2px dashed var(--border);
    border-radius: 12px;
    padding: 48px 24px;
    text-align: center;
    cursor: pointer;
    background: var(--panel);
    transition: background 0.1s, border-color 0.1s;
    display: flex;
    flex-direction: column;
    gap: 8px;
    align-items: center;
  }
  .dropzone.over {
    border-color: var(--accent);
    background: var(--panel-2);
  }
  .dropzone p { margin: 0; color: var(--muted); }
  .hint { font-size: 12.5px; max-width: 60ch; }
  code { background: var(--panel-2); padding: 1px 6px; border-radius: 4px; }
  .error { color: var(--bad); margin-top: 8px; }
</style>
