<script lang="ts">
  let { text, label = 'Copy' }: { text: string; label?: string } = $props();

  let copied = $state(false);
  let timer: ReturnType<typeof setTimeout> | undefined;

  async function copy(): Promise<void> {
    try {
      await navigator.clipboard.writeText(text);
    } catch {
      // navigator.clipboard needs a secure context (https / loopback). On a
      // plain-http LAN origin fall back to the legacy textarea trick.
      const ta = document.createElement('textarea');
      ta.value = text;
      ta.style.position = 'fixed';
      ta.style.opacity = '0';
      document.body.appendChild(ta);
      ta.select();
      try { document.execCommand('copy'); } finally { ta.remove(); }
    }
    copied = true;
    clearTimeout(timer);
    timer = setTimeout(() => (copied = false), 1400);
  }
</script>

<button class="copybtn" class:copied onclick={copy} aria-label={`Copy command: ${text}`}>
  {copied ? 'Copied ✓' : label}
</button>

<style>
  .copybtn {
    appearance: none;
    background: var(--panel-2);
    border: 1px solid var(--border);
    border-radius: 6px;
    color: var(--muted);
    font-size: 11.5px;
    padding: 0 10px;
    cursor: pointer;
    white-space: nowrap;
    transition: color 120ms ease, background 120ms ease;
  }
  .copybtn:hover { color: var(--accent); background: var(--panel); }
  .copybtn.copied { color: var(--ok, #7dd97d); }
</style>
