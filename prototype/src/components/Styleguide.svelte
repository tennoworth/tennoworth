<script lang="ts">
  import { onMount } from 'svelte';
  import { applyTheme, systemMode, type Mode } from '../lib/theme';

  let mode = $state<Mode>('dark');
  let filter = $state('');
  let notificationRead = $state(false);
  let dataState = $state('populated');
  let selected = $state('Fast Cash');
  let quantity = $state<number | undefined>(2);
  let message = $state('');
  let dialog: HTMLDialogElement;
  let swatches = $state<{ token: string; value: string }[]>([]);
  const tokens = ['--bg', '--panel', '--panel-2', '--fg', '--muted', '--border', '--accent', '--good', '--warn', '--bad', '--ducat'];
  const rows = [
    { name: 'Ivara Prime Neuroptics Blueprint', owned: 4, price: 15, note: '2 copies kept · sample inventory' },
    { name: 'Pyrana Prime Set', owned: 30, price: 95, note: 'Existing visible order · sample inventory' },
    { name: 'A deliberately long item identity that must remain readable when the window becomes narrow', owned: 1, price: null, note: 'Price unavailable; not zero' },
  ];
  const modes = ['Fast Cash', 'Plat per Trade', 'Clear Inventory', 'Max Value'];
  let visibleRows = $derived(rows.filter(row => row.name.toLowerCase().includes(filter.toLowerCase())));
  let invalidQuantity = $derived(!Number.isInteger(quantity) || (quantity ?? 0) < 1 || (quantity ?? 0) > 4);

  function pickTheme(next: Mode) {
    mode = next;
    applyTheme(next);
    const styles = getComputedStyle(document.documentElement);
    swatches = tokens.map(token => ({ token, value: styles.getPropertyValue(token).trim() }));
  }

  onMount(() => pickTheme(systemMode()));
</script>

<main class="styleguide ui-stack">
  <header class="ui-stack">
    <div class="ui-toolbar">
      <span class="edition">TennoWorth / Design system v1</span>
      <div class="ui-toolbar" role="group" aria-label="Preview theme">
        {#each ['light', 'dark'] as choice}
          <button class="btn" class:primary={mode === choice} aria-pressed={mode === choice} onclick={() => pickTheme(choice as Mode)}>{choice === 'light' ? 'Light' : 'Dark'}</button>
        {/each}
      </div>
      <a class="btn ghost" href="/?preview-desktop&sample=session">Open sample app</a>
    </div>
    <h1>One visual language. Room for the information.</h1>
    <p class="intro">Production styles, exercised with fictional data. Resize this window and try the controls. Nothing here changes inventory, settings, or market orders.</p>
  </header>

  <section class="wrap tw" aria-labelledby="palette-title">
    <div class="rail"><h3 id="palette-title">01 / Paper, ink, and meaning</h3><span class="exp">Values read from the active stylesheet</span></div>
    <div class="swatches">
      {#each swatches as swatch}
        <div class="swatch"><span class="chip" style:background={`var(${swatch.token})`} aria-hidden="true"></span><code>{swatch.token}</code><span class="muted">{swatch.value}</span></div>
      {/each}
    </div>
    <div class="specimens ui-stack">
      <h2>Archivo Narrow / clear headings</h2>
      <p>IBM Plex Sans / Long item names, explanations, and important caveats deserve enough space to read.</p>
      <p class="numeric">IBM Plex Mono / 1,250 p · 36 trades · 12:45 UTC</p>
    </div>
  </section>

  <section class="wrap tw" aria-labelledby="controls-title">
    <div class="rail"><h3 id="controls-title">02 / Controls and intent</h3><span class="exp">Tab through to inspect focus; hover to inspect feedback</span></div>
    <div class="specimens ui-stack">
      <div class="ui-toolbar">
        <button class="btn primary lg" onclick={() => dialog.showModal()}>Review sample listing</button>
        <button class="btn" onclick={() => { filter = ''; dataState = 'populated'; message = 'Sample filters reset.'; }}>Reset filters</button>
        <button class="btn ghost" onclick={() => message = 'Secondary action selected. No account request was made.'}>Secondary action</button>
        <button class="btn" disabled>Unavailable</button>
        <button class="btn bad" onclick={() => message = 'Destructive actions need a contextual confirmation before changing data.'}>Destructive action example</button>
      </div>
      <div class="mode-options" role="group" aria-label="Planning intent examples">
        {#each modes as item}
          <button class="btn" class:primary={selected === item} aria-pressed={selected === item} onclick={() => selected = item}>{item}</button>
        {/each}
      </div>
      <p class="muted">Selected: {selected}. These are presentation examples, not the Trade Session planner.</p>
      <div class="fields">
        <label class="ui-field">Filter sample items<input type="text" bind:value={filter} placeholder="Item name" /></label>
        <label class="ui-field">Data state<select bind:value={dataState}><option value="populated">Populated</option><option value="empty">Empty</option><option value="loading">Loading</option><option value="error">Error</option><option value="notifications">Alerts</option></select></label>
      </div>
      <p role="status" class="muted">{message || 'Controls are ready. No changes made.'}</p>
    </div>
  </section>

  <section class="wrap tw" aria-labelledby="table-title">
    <div class="rail"><h3 id="table-title">03 / Dense data, complete names</h3><span class="exp">The table scrolls locally; the page does not</span></div>
    {#if dataState === 'loading'}
      <div class="specimens" role="status" aria-busy="true">Loading sample inventory…</div>
    {:else if dataState === 'error'}
      <div class="specimens ui-stack"><p class="ui-notice" data-tone="bad" role="alert">Inventory could not be loaded. No saved items were removed.</p><div><button class="btn" onclick={() => dataState = 'populated'}>Retry sample load</button></div></div>
    {:else if dataState === 'empty' || visibleRows.length === 0}
      <div class="specimens ui-stack"><h2>{dataState === 'empty' ? 'No inventory yet' : 'No matching items'}</h2><p>{dataState === 'empty' ? 'Scan the game to see owned items. This reference uses sample data only.' : 'Try a shorter name or reset the filter.'}</p><div><button class="btn" onclick={() => { dataState = 'populated'; filter = ''; }}>Show sample inventory</button></div></div>
    {:else}
      <!-- svelte-ignore a11y_no_noninteractive_tabindex (Keyboard users must be able to scroll the wide table.) -->
      <div class="scroll" tabindex="0" role="region" aria-label="Sample inventory table">
        <table class="tw fixed reference-table"><thead><tr><th scope="col" class="l">Item</th><th scope="col">Owned</th><th scope="col">Unit price</th><th scope="col" class="l">Context</th></tr></thead><tbody>
          {#each visibleRows as row}
            <tr><td class="l identity">{row.name}</td><td>{row.owned}</td><td>{row.price === null ? 'Unknown' : `${row.price} p`}</td><td class="reason context">{row.note}</td></tr>
          {/each}
        </tbody></table>
      </div>
    {/if}
    <div class="line">Sample data only · missing prices stay unknown · quantities are not recommendations</div>
  </section>

  <section class="wrap tw" aria-labelledby="states-title">
    <div class="rail"><h3 id="states-title">04 / Meaning without color dependence</h3></div>
    <div class="specimens ui-stack">
      <p class="ui-notice" data-tone="good"><strong>Saved.</strong> The sample draft is ready for review.</p>
      <p class="ui-notice" data-tone="warn"><strong>Estimated allowance.</strong> Trades may have happened while monitoring was unavailable. Scan again to confirm the remaining count.</p>
      <p class="ui-notice" data-tone="bad"><strong>Could not refresh prices.</strong> Last known values remain visible; check their age before listing.</p>
      <p class="ui-notice"><strong>Unknown is not zero.</strong> A missing market observation cannot support a price claim.</p>
    </div>
  </section>
  <section class="ui-panel" aria-labelledby="panel-title">
    <header><h2 id="panel-title">Shared panel header</h2><span class="muted">Used by management screens</span></header>
    <p>Watches and Ledger use this panel shell. The heading and adjacent status wrap together, with shared spacing and no feature-specific card copy.</p>
  </section>
  <footer class="muted">Reference patterns: .btn · .wrap.tw · table.tw · .ui-field · .ui-toolbar · .ui-notice · dialog.cryptobox</footer>
  {#if dataState === 'notifications'}
  <section class="ui-panel ui-stack" aria-label="Notification row reference">
    <h2>Notification history</h2>
    <article class="notification-entry" class:unread={!notificationRead}>
      <span>{notificationRead ? 'Read' : 'Unread'} · Completed trade</span>
      <strong>Sold Pyrana Prime Set for 90p</strong>
      <p>Your completed trade is saved in the Ledger. Review My Orders if the listing still needs adjustment.</p>
      <div class="ui-notice" data-tone="warn">Desktop delivery failed. The inbox keeps the result and next action.</div>
      <div><button class="btn" onclick={() => notificationRead = !notificationRead}>{notificationRead ? 'Mark unread' : 'Mark read'}</button></div>
    </article>
  </section>
  {/if}
</main>

<dialog class="cryptobox" bind:this={dialog} aria-labelledby="review-title">
  <form method="dialog" onsubmit={() => message = 'Sample review closed. No listing was submitted.'}>
    <header><h3 id="review-title">Review sample listing</h3><p>Ivara Prime Neuroptics Blueprint</p></header>
    <p class="ui-notice" data-tone="warn">Existing visible order: 4 copies → {quantity ?? '—'} copies. This replaces the total; it does not add copies.</p>
    <label class="ui-field">Total quantity<input type="number" min="1" max="4" step="1" bind:value={quantity} aria-invalid={invalidQuantity} aria-describedby="quantity-help" /></label>
    <p id="quantity-help" class="muted">{invalidQuantity ? 'Enter a whole quantity from 1 to 4.' : 'Up to 4 safe copies. Resize while editing; your draft stays intact.'}</p>
    <footer><button class="btn" value="cancel" formnovalidate>Cancel</button><button class="btn primary" value="confirm" disabled={invalidQuantity}>Confirm sample</button></footer>
  </form>
</dialog>

<style>
  .styleguide { max-width: 76rem; margin: 0 auto; padding: var(--s5) var(--gutter); gap: var(--s5); }
  .styleguide > * { flex-shrink: 0; }
  .edition { flex: 1 1 20rem; font: 600 var(--text-caption)/var(--leading-body) var(--font-ui); letter-spacing: .12em; text-transform: uppercase; }
  h1 { margin: 0; font-size: clamp(1.5rem, 4vw, 2.25rem); line-height: 1.2; font-weight: 600; }
  h2 { margin: 0; font-size: var(--text-heading); font-weight: 600; }
  p { margin: 0; }
  .intro { max-width: 76ch; color: var(--muted); }
  .specimens { padding: var(--inset); }
  .numeric, code { font-family: var(--font-mono); }
  .swatches { display: grid; grid-template-columns: repeat(auto-fit, minmax(min(100%, 12rem), 1fr)); gap: var(--s3); padding: var(--inset); border-bottom: 1px var(--rule) var(--hairline); }
  .swatch { display: grid; grid-template-columns: var(--s6) minmax(0, 1fr); gap: var(--s1) var(--s2); align-items: center; }
  .swatch .muted { grid-column: 2; overflow-wrap: anywhere; }
  .chip { width: var(--s6); height: var(--s6); grid-row: span 2; border: 1px solid var(--border); }
  .fields { display: grid; grid-template-columns: repeat(auto-fit, minmax(min(100%, 16rem), 1fr)); gap: var(--s3); }
  .mode-options { display: grid; grid-template-columns: repeat(auto-fit, minmax(min(100%, 12rem), 1fr)); gap: var(--s2); }
  .reference-table { min-width: 44rem; table-layout: fixed; }
  .reference-table th:first-child { width: 42%; }
  .reference-table th:last-child { width: 30%; }
  .reference-table td.identity, .reference-table td.context { white-space: normal; overflow-wrap: anywhere; padding-block: var(--s2); }
</style>
