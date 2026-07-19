<script lang="ts">
  import {
    buildAssistantContext, askAssistant, assistantErrorMessage,
    type AssistantSourceRow,
  } from '../lib/assistant';
  import type { CompanionConfig } from '../lib/types';

  interface ChatMessage {
    role: 'user' | 'assistant';
    content: string;
  }

  interface Props {
    rows: AssistantSourceRow[];
    /** Same pre-formatted "X ago" string as the app's own market-data
     *  staleness indicator (App.svelte's `marketStaleness`) — reused
     *  verbatim, not recomputed here. */
    marketAge?: string | null;
    config: CompanionConfig | null;
    companionStatus: 'unchecked' | 'connecting' | 'connected' | 'error';
  }
  let { rows, marketAge = null, config, companionStatus }: Props = $props();

  const STARTERS = [
    'What should I sell today?',
    'I need 200 platinum fast — quickest path?',
    "What's my inventory worth?",
  ];

  // Sent history is capped to the last 12 messages inside askAssistant() —
  // the displayed transcript stays full since it's in-memory-only anyway.
  let open = $state(false);
  let question = $state('');
  let messages = $state<ChatMessage[]>([]);
  let pending = $state(false);
  let errorMsg = $state<string | null>(null);
  let listEl: HTMLDivElement | undefined = $state();

  let connected = $derived(companionStatus === 'connected' && config !== null);
  let hasRows = $derived(Array.isArray(rows) && rows.length > 0);
  let disabledReason = $derived.by(() => {
    if (!connected) return 'Connect the companion to use the advisor.';
    if (!hasRows) return 'Load your inventory first.';
    return null;
  });
  let inputDisabled = $derived(disabledReason !== null || pending);
  let canSend = $derived(!inputDisabled && question.trim().length > 0);

  function toggle(): void {
    open = !open;
  }
  function close(): void {
    open = false;
  }

  // Esc closes — only listens while the drawer is actually open.
  $effect(() => {
    if (!open) return;
    const handler = (e: KeyboardEvent): void => {
      if (e.key === 'Escape') close();
    };
    document.addEventListener('keydown', handler);
    return () => document.removeEventListener('keydown', handler);
  });

  // Keep the transcript scrolled to the newest message. Reads reactive
  // state but only writes a DOM property (not $state), so this can't
  // re-trigger itself.
  $effect(() => {
    messages.length; pending; errorMsg;
    if (listEl) listEl.scrollTop = listEl.scrollHeight;
  });

  async function send(text: string): Promise<void> {
    const trimmed = text.trim();
    if (!trimmed || inputDisabled || !config) return;
    const historySnapshot = messages.map((m) => ({ role: m.role, content: m.content }));
    messages.push({ role: 'user', content: trimmed });
    question = '';
    pending = true;
    errorMsg = null;
    const context = buildAssistantContext(rows, { generatedAt: marketAge });
    try {
      const resp = await askAssistant(config, trimmed, historySnapshot, context);
      messages.push({ role: 'assistant', content: resp.answer });
    } catch (e) {
      errorMsg = assistantErrorMessage(e);
    } finally {
      pending = false;
    }
  }

  function onSubmit(e: SubmitEvent): void {
    e.preventDefault();
    void send(question);
  }

  function onKeydown(e: KeyboardEvent): void {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      void send(question);
    }
  }

  function askStarter(text: string): void {
    void send(text);
  }
</script>

<button
  type="button"
  class="fab"
  onclick={toggle}
  aria-label={open ? 'Close AI advisor chat' : 'Open AI advisor chat'}
  title="AI advisor"
>
  <svg viewBox="0 0 24 24" width="22" height="22" aria-hidden="true">
    <path
      d="M4 5.5A2.5 2.5 0 0 1 6.5 3h11A2.5 2.5 0 0 1 20 5.5v8A2.5 2.5 0 0 1 17.5 16H9l-4.2 3.5a.5.5 0 0 1-.8-.4V16h-.5A2.5 2.5 0 0 1 1 13.5v0"
      fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"
    />
  </svg>
</button>

{#if open}
  <div class="backdrop" onclick={close} role="presentation"></div>
  <div class="drawer" role="dialog" aria-modal="true" aria-labelledby="assistant-title">
    <header>
      <h2 id="assistant-title">AI advisor</h2>
      <button class="x" onclick={close} aria-label="Close">×</button>
    </header>

    <div class="staleness">
      Market data: {marketAge ?? 'unknown'}
    </div>

    <div class="messages" bind:this={listEl}>
      {#if messages.length === 0}
        <p class="hint">Ask about what to sell, fast plat, or your inventory's worth.</p>
        <div class="chips">
          {#each STARTERS as s (s)}
            <button type="button" class="chip" onclick={() => askStarter(s)} disabled={inputDisabled}>
              {s}
            </button>
          {/each}
        </div>
      {/if}
      {#each messages as m, i (i)}
        <div class="bubble {m.role}">
          <p>{m.content}</p>
        </div>
      {/each}
      {#if pending}
        <div class="bubble assistant pending">
          <p>Thinking…</p>
        </div>
      {/if}
      {#if errorMsg}
        <div class="bubble assistant error">
          <p>{errorMsg}</p>
        </div>
      {/if}
    </div>

    {#if disabledReason}
      <div class="disabled-hint">{disabledReason}</div>
    {/if}

    <!-- Point-of-use disclosure: the app's promise is "your data stays in the
         tab", and this drawer is the one exception — say so where it's used,
         not only in the FAQ. -->
    <p class="privacy-note">
      Sends the rows shown in your sell table (after filters) — names,
      owned/sellable counts, prices, 48-hour volume, and vault status — plus
      totals and the market age, with your question, to DeepSeek. Never your
      full inventory, account, or the companion token.
    </p>

    <form class="composer" onsubmit={onSubmit}>
      <textarea
        bind:value={question}
        placeholder="Ask the advisor… (Enter to send, Shift+Enter for a new line)"
        rows="2"
        disabled={inputDisabled}
        onkeydown={onKeydown}
      ></textarea>
      <button type="submit" disabled={!canSend}>{pending ? 'Sending…' : 'Send'}</button>
    </form>
  </div>
{/if}

<style>
  .fab {
    position: fixed;
    right: 20px;
    bottom: 20px;
    width: 48px;
    height: 48px;
    border-radius: 50%;
    display: grid;
    place-items: center;
    background: var(--accent);
    color: var(--bg);
    border: none;
    cursor: pointer;
    box-shadow: 0 6px 18px rgba(0, 0, 0, 0.45);
    z-index: 900;
    transition: transform 120ms ease, box-shadow 120ms ease;
  }
  .fab:hover {
    transform: scale(1.05);
    box-shadow: 0 8px 22px rgba(0, 0, 0, 0.55);
  }

  .backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.45);
    z-index: 940;
  }

  .drawer {
    position: fixed;
    top: 0;
    right: 0;
    bottom: 0;
    width: min(400px, 100vw);
    background: var(--panel);
    border-left: 1px solid var(--border);
    box-shadow: -8px 0 24px rgba(0, 0, 0, 0.45);
    z-index: 950;
    display: flex;
    flex-direction: column;
  }

  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 14px 18px;
    border-bottom: 1px solid var(--border);
    flex-shrink: 0;
  }
  header h2 {
    margin: 0;
    font-size: 13px;
    letter-spacing: 0.05em;
    text-transform: uppercase;
    color: var(--accent);
    font-weight: 600;
  }
  .x {
    background: transparent;
    border: 1px solid var(--border);
    color: var(--muted);
    font-size: 16px;
    line-height: 1;
    width: 26px;
    height: 26px;
    border-radius: 6px;
    cursor: pointer;
  }
  .x:hover { color: var(--fg); }

  .staleness {
    padding: 8px 18px;
    font-size: 11.5px;
    color: var(--muted);
    border-bottom: 1px solid var(--border);
    flex-shrink: 0;
  }

  .messages {
    flex: 1;
    overflow-y: auto;
    padding: 14px 18px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .hint {
    margin: 0 0 4px;
    font-size: 12.5px;
    color: var(--muted);
    line-height: 1.5;
  }
  .chips {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .chip {
    text-align: left;
    background: var(--panel-2);
    border: 1px solid var(--border);
    color: var(--fg);
    font-size: 12.5px;
    padding: 8px 10px;
    border-radius: 8px;
    cursor: pointer;
    transition: border-color 120ms ease, color 120ms ease;
  }
  .chip:hover:not(:disabled) { border-color: var(--accent); color: var(--accent); }
  .chip:disabled { opacity: 0.4; cursor: not-allowed; }

  .bubble {
    max-width: 88%;
    padding: 8px 12px;
    border-radius: 10px;
    font-size: 13px;
    line-height: 1.5;
  }
  .bubble p {
    margin: 0;
    white-space: pre-wrap;
    word-break: break-word;
  }
  .bubble.user {
    align-self: flex-end;
    background: var(--accent);
    color: var(--bg);
    border-bottom-right-radius: 3px;
  }
  .bubble.assistant {
    align-self: flex-start;
    background: var(--panel-2);
    border: 1px solid var(--border);
    color: var(--fg);
    border-bottom-left-radius: 3px;
  }
  .bubble.assistant.pending { color: var(--muted); }
  .bubble.assistant.error {
    color: var(--bad);
    border-color: color-mix(in srgb, var(--bad) 35%, var(--border));
  }

  .disabled-hint {
    padding: 8px 18px;
    font-size: 12px;
    color: var(--warn);
    border-top: 1px solid var(--border);
    flex-shrink: 0;
  }

  .privacy-note {
    margin: 0;
    padding: 8px 18px 0;
    font-size: 11px;
    line-height: 1.4;
    color: var(--muted);
    flex-shrink: 0;
  }

  .composer {
    display: flex;
    gap: 8px;
    padding: 12px 18px;
    border-top: 1px solid var(--border);
    flex-shrink: 0;
  }
  .composer textarea {
    flex: 1;
    resize: none;
    font: inherit;
    font-size: 12.5px;
    background: var(--panel-2);
    border: 1px solid var(--border);
    color: var(--fg);
    border-radius: 8px;
    padding: 8px 10px;
  }
  .composer textarea:disabled { opacity: 0.5; }
  .composer button {
    align-self: flex-end;
    background: var(--accent);
    color: var(--bg);
    border: none;
    font-weight: 600;
    font-size: 12.5px;
    padding: 8px 14px;
    border-radius: 8px;
    cursor: pointer;
  }
  .composer button:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }

  @media (max-width: 480px) {
    .drawer { width: 100vw; }
  }
</style>
