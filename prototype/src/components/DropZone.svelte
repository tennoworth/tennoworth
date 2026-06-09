<script lang="ts">
  interface Props {
    loading?: boolean;
    oninventory?: (event: { name: string; data: unknown }) => void;
  }
  let { loading = false, oninventory }: Props = $props();

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
      const msg = e instanceof Error ? e.message : String(e);
      parseError = `Couldn't parse ${file.name} as JSON: ${msg}`;
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
    <p class="hint">
      Don't have one yet? Run the
      <a href="#companion">companion CLI</a>
      with Warframe open — it extracts <code>inventory.json</code> from your
      running game and saves it next to itself. Already have one from another
      tool (AlecaFrame export, Sainan's <code>warframe-api-helper</code>)? It
      drops in here too.
    </p>
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
