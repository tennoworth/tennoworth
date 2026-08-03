<script lang="ts">
  export interface ToastMessage {
    id: number;
    kind: 'error' | 'success';
    text: string;
  }

  let {
    toasts = [],
    ondismiss,
  }: {
    toasts?: ToastMessage[];
    ondismiss?: (id: number) => void;
  } = $props();
</script>

{#if toasts.length > 0}
  <div class="toast-stack" role="status" aria-live="polite">
    {#each toasts as t (t.id)}
      <div class="toast {t.kind}">
        <span class="toast-text">{t.text}</span>
        <button class="toast-dismiss" onclick={() => ondismiss?.(t.id)} aria-label="Dismiss">×</button>
      </div>
    {/each}
  </div>
{/if}

<style>
  .toast-stack {
    position: fixed;
    bottom: 18px;
    right: 18px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    z-index: 1100;
    max-width: min(380px, calc(100vw - 36px));
  }
  .toast {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 10px;
    background: var(--panel-2);
    border: 1px solid var(--border);
    border-left: 3px solid var(--accent);
    border-radius: 8px;
    padding: 10px 12px;
    box-shadow: 0 12px 28px rgba(0, 0, 0, 0.5);
    font-size: 12.5px;
    line-height: 1.5;
    color: var(--fg);
  }
  .toast.error { border-left-color: var(--bad); }
  .toast.success { border-left-color: var(--good); }
  .toast-text { min-width: 0; }
  .toast-dismiss {
    background: transparent;
    border: none;
    color: var(--muted);
    font-size: 15px;
    line-height: 1;
    cursor: pointer;
    padding: 2px 3px;
    flex-shrink: 0;
  }
  .toast-dismiss:hover { color: var(--fg); }
</style>
