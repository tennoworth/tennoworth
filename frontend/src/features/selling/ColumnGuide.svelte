<script lang="ts" module>
  export interface GuideEntry { key: string; label: string; text: string; unit?: string; dir?: string; }
</script>

<script lang="ts">
  // One shared explanation of the visible columns. It replaced a 14px "?" per
  // header that floated over the neighbouring column and could not share a
  // header with a real sort button.
  let { entries }: { entries: GuideEntry[] } = $props();

  let dialog: HTMLDialogElement;
  let opener: HTMLElement | null = null;

  function open(event: MouseEvent): void {
    opener = event.currentTarget as HTMLElement;
    dialog.showModal();
  }
  function restoreFocus(): void {
    if (opener?.isConnected) opener.focus();
  }
  function containFocus(event: KeyboardEvent): void {
    if (event.key !== 'Tab') return;
    const controls = [...dialog.querySelectorAll<HTMLElement>('button:not(:disabled), [tabindex="0"]')].filter((el) => el.getClientRects().length > 0);
    const first = controls[0], last = controls.at(-1);
    const outside = !controls.includes(document.activeElement as HTMLElement);
    if (outside || (event.shiftKey ? document.activeElement === first : document.activeElement === last)) {
      event.preventDefault();
      (event.shiftKey ? last : first)?.focus();
    }
  }
</script>

<button type="button" class="btn" aria-haspopup="dialog" onclick={open}>Column guide</button>
<dialog bind:this={dialog} class="cryptobox column-guide" aria-labelledby="column-guide-title" onkeydown={containFocus} onclose={restoreFocus}>
  <form method="dialog">
    <header>
      <h3 id="column-guide-title">Column guide</h3>
      <button class="btn">Close</button>
    </header>
    <dl>
      {#each entries as entry (entry.key)}
        <div class="entry">
          <dt>{entry.label}</dt>
          <dd>
            {entry.text}
            {#if entry.unit || entry.dir}
              <span class="meta">
                {#if entry.unit}<span class="meta-key">unit</span> {entry.unit}{/if}
                {#if entry.unit && entry.dir} · {/if}
                {#if entry.dir}<span class="meta-key">direction</span> {entry.dir}{/if}
              </span>
            {/if}
          </dd>
        </div>
      {/each}
    </dl>
  </form>
</dialog>

<style>
  /* Rendered inside the table toolbar, whose nowrap would otherwise reach it. */
  dialog.column-guide { max-width: 44rem; overflow: hidden; white-space: normal; }
  dialog.column-guide[open] { display: flex; flex-direction: column; }
  /* Anchored on the dialog's class so these outrank the shared cryptobox rules,
     which stack a form header's title over its description. */
  .column-guide form { min-height: 0; flex: 1; }
  .column-guide header { flex-direction: row; align-items: center; justify-content: space-between; gap: var(--s3); text-align: left; }
  .column-guide h3 { flex: 1; }
  /* The header is a title rail, so its button takes the rail's pair. */
  .column-guide header .btn { color: var(--rail-fg); background: transparent; border-color: var(--rail-muted); }
  dl { margin: 0; overflow-y: auto; min-height: 0; }
  .entry {
    display: grid;
    grid-template-columns: minmax(0, 8rem) minmax(0, 1fr);
    gap: var(--s3);
    padding-block: var(--s2);
    border-bottom: 1px var(--rule) var(--hairline);
  }
  .entry:first-child { padding-top: 0; }
  .entry:last-child { border-bottom: 0; }
  dt {
    font: 600 var(--text-caption)/var(--leading-body) var(--font-ui);
    letter-spacing: 0.12em;
    text-transform: uppercase;
    color: var(--fg);
  }
  dd { margin: 0; font-size: var(--text-control); line-height: var(--leading-body); color: var(--fg); }
  .meta { display: block; margin-top: var(--s1); color: var(--muted); font-size: var(--text-caption); }
  .meta-key { font-family: var(--font-ui); letter-spacing: 0.08em; text-transform: uppercase; }
  @media (max-width: 30rem) {
    .entry { grid-template-columns: minmax(0, 1fr); gap: var(--s1); }
  }
</style>
