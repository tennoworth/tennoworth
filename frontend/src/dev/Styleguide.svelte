<script lang="ts">
  import NotificationRow from '../features/settings/NotificationRow.svelte';
  import baroNotice from '../../../tests/fixtures/notifications/baro.json';
  import type { NotificationEntry } from '../contracts/desktop';
  import UsageChart from '../features/community/UsageChart.svelte';
  import usageSample from '../../../tests/fixtures/usage/daily.json';
  import UpdateNotes from '../ui/UpdateNotes.svelte';
  import WfmTokenGuide from '../features/settings/WfmTokenGuide.svelte';
  import notesSample from '../../../tests/fixtures/update-notes/status.json';
  import type { UpdateNotesStatus } from '../contracts/update';
  import ResultsTable from '../features/selling/ResultsTable.svelte';
  import type { SellRow } from '../contracts/selling';
  import PromptBanner from '../ui/PromptBanner.svelte';
  import type { SettingsStore } from '../contracts/state-store';
  import { PromptSession } from '../ui/prompt-session.svelte';
  import AppIconSetting from '../features/settings/AppIconSetting.svelte';
  import { shimmer } from '../ui/shimmer';
  import { AppIconController } from '../features/settings/app-icon.svelte';
  import PresenceSettings from '../features/presence/PresenceSettings.svelte';
  import PresenceMark from '../features/presence/PresenceMark.svelte';
  import { PresenceController } from '../features/presence/presence.svelte';
  import type { PresenceStatus } from '../contracts/desktop';
  let notesRef = $state<UpdateNotes>();
  // A fresh, always-due session each time the state is chosen; nothing persists.
  const promptStore: SettingsStore = { mode: 'local', hydrate: async () => {}, getSetting: () => null, setSetting: async () => {} };
  const samplePromptSession = () => untrack(() => {
    const session = new PromptSession(promptStore, [{ id: 'styleguide-prompt', delayDays: 0, minLaunches: 1, gapDays: 0, maxAsks: 1 }]);
    session.launch();
    session.notes('none');
    return session;
  });
  // Fictional rows for the production Sell table: sortable header buttons and
  // the shared Column guide.
  const sellRow = (name: string, slug: string, owned: number, low: number, vol: number, score: number): SellRow => ({
    key: slug, slug, subtype: null, name, owned, sellable: owned, leveled: 0, type: 'Upgrades', kept_lvl: null,
    ducats: null, plat_per_100d: null, avg_price: low + 1, low_sell: low, low5_avg: low, top_buy: low - 2,
    volume_48h: vol, ratio: 1, potential_plat: owned * (low + 1), raw_value: owned * low, sell_score: score,
    patience: false, timing: 'neutral', medians_7d: [low, low + 1, low], median_90d: low, delta_90d_pct: 0, tags: [],
    is_augment: false, vault_status: 'available',
  });
  const sellSample = [sellRow('Primed Flow', 'primed_flow', 6, 23, 107, 138), sellRow('Arcane Energize', 'arcane_energize', 12, 7, 362, 84), sellRow('Sample long item name for wrapping', 'sample_long', 4, 4, 68, 18)];
  const notesServices = { updateNotes: async () => ({ ...notesSample, auto_show: false } as UpdateNotesStatus), acknowledgeUpdateNotes: async () => {}, updateNotesCanPresent: async () => true };
  import { onMount, untrack } from 'svelte';
  import { applyTheme, systemMode, type Mode } from '../ui/theme';

  let mode = $state<Mode>('dark');
  // The production App icon row over a store and native side that keep nothing.
  const sampleAppIcon = new AppIconController({
    store: { mode: 'local', hydrate: async () => {}, getSetting: () => null, setSetting: async () => {} },
    theme: { get mode() { return mode; }, subscribe: () => () => {} },
    native: { setAppIcon: async () => ({ windowIcon: true }) },
  });
  // The production Trade presence group over a native side that keeps the pick
  // in memory; picking a status pauses following, as it does in the app.
  let samplePresenceStatus: PresenceStatus = {
    signedIn: true, connected: true, status: 'ingame', statusUntil: null, statusSetAt: '2026-10-09T12:02:00Z',
    managed: true, following: true, followPaused: false, gameRunning: true, problem: null, detail: null,
    settings: { followGame: true, whenClosed: 'invisible', keepForMinutes: null },
  };
  const samplePresence = new PresenceController({
    native: {
      presenceStatus: async () => samplePresenceStatus,
      setPresence: async (status) => (samplePresenceStatus = { ...samplePresenceStatus, status, managed: false, following: false, followPaused: true }),
      updatePresenceSettings: async (settings) => settings,
      followGameNow: async () => { samplePresenceStatus = { ...samplePresenceStatus, status: 'ingame', following: true, followPaused: false }; void samplePresence.refresh(); },
    },
    listen: () => () => {},
  });
  void samplePresence.refresh();
  let filter = $state('');
  let notificationRead = $state(false);
  let exampleEnabled = $state(true);
  let exampleShortcut = $state('Ctrl+Shift+O');
  let dataState = $state('populated');
  let shimmerPulse = $state(0);
  let selected = $state('Fast Cash');
  let quantity = $state<number | undefined>(2);
  let message = $state('');
  let dialog: HTMLDialogElement;
  let tokenDialog: HTMLDialogElement;
  let swatches = $state<{ token: string; value: string }[]>([]);
  const tokens = ['--bg', '--panel', '--panel-2', '--fg', '--muted', '--border', '--accent', '--good', '--warn', '--bad', '--ducat', '--vault'];
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
        <label class="ui-field">Data state<select bind:value={dataState}><option value="populated">Populated</option><option value="empty">Empty</option><option value="loading">Loading</option><option value="error">Error</option><option value="notifications">Alerts</option><option value="estimates">Estimates</option><option value="usage">Community usage</option><option value="updates">Update notes</option><option value="token">WFM token sign-in</option><option value="sell">Sell table</option><option value="prompt">Prompt banner</option><option value="app-icon">App icon</option><option value="shimmer">Field shimmer</option><option value="presence">Trade presence</option></select></label>
      </div>
      <p role="status" class="muted">{message || 'Controls are ready. No changes made.'}</p>
    </div>
  </section>

  <section class="wrap tw" aria-labelledby="table-title">
    <div class="rail"><h3 id="table-title">03 / Dense data, complete names</h3><span class="exp">The table scrolls locally; the page does not</span></div>
    {#if dataState === 'estimates'}
      <div class="specimens ui-stack"><h2>Estimated opportunities</h2><p class="numeric">Known estimated value: 30p</p><p class="ui-notice" data-tone="good">✓ Keep rules applied to this inventory. Existing WFM listings still need checking.</p><p class="ui-notice" data-tone="bad"><strong>! We can’t calculate what you can sell yet</strong> Invalid inventory quantities block sale totals. Scan game again.</p><p class="ui-notice" data-tone="warn">Estimates honor inventory protection. Current WFM listings are not accounted for. Quantities unavailable for 1 item; known totals exclude it.</p><div class="ui-toolbar"><button class="btn" disabled>List on WFM</button><button class="btn primary" onclick={() => message = 'Recheck protection before listing. Imported backups require a game scan; review edits are kept.'}>Recheck protection</button></div></div>
    {:else if dataState === 'loading'}
      <div class="specimens" role="status" aria-busy="true">Loading sample inventory…</div>
    {:else if dataState === 'error'}
      <div class="specimens ui-stack"><div class="ui-toolbar"><strong>Using saved scan</strong><time datetime="2026-09-08T12:00:00Z">As of 8 September 2026, 12:00 UTC · 2d ago</time><span class="bad">Last refresh failed</span></div><p class="ui-notice" data-tone="warn">Alternative outcome: <strong>No tradeable items found.</strong> Showing saved inventory; this scan did not replace its quantities.</p><p class="ui-notice" data-tone="bad" role="alert">Inventory could not be refreshed. Saved items remain available.</p><div><button class="btn" onclick={() => dataState = 'populated'}>Retry sample load</button></div></div>
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
  <section class="wrap tw ui-reading" aria-labelledby="panel-title">
    <div class="rail"><h3 id="panel-title">05 / Aligned preferences</h3></div>
    <div class="ui-setting-row"><div class="ui-setting-copy"><label for="example-enabled">Local recognition example</label><p>Labels and help text share one edge; related controls share another.</p></div><label class="ui-setting-check"><input id="example-enabled" type="checkbox" bind:checked={exampleEnabled} />Enabled</label></div>
    <div class="ui-setting-row"><div class="ui-setting-copy"><label for="example-shortcut">Retry shortcut example</label><p>Resize or change theme while editing. The draft stays in place.</p></div><div class="ui-setting-control"><input id="example-shortcut" class="ui-input" bind:value={exampleShortcut} disabled={!exampleEnabled} /></div></div>
    <div class="ui-panel-footer">These examples do not change desktop preferences.</div>
  </section>
  <footer class="muted">Reference patterns: .btn · .wrap.tw · table.tw · .ui-field · .ui-toolbar · .ui-notice · dialog.cryptobox</footer>
  {#if dataState === 'notifications'}
  <section class="ui-panel ui-stack" aria-label="Notification row reference">
    <h2>Notification history</h2>
    <NotificationRow entry={{ id: 1, category: 'trades', title: 'Sold Pyrana Prime Set for 90p', body: 'Your completed trade is saved in the Ledger. Review My Orders if the listing still needs adjustment.', target: 'orders', created_at: Date.now() / 1000, read: notificationRead, delivery: 'failed' }} now={Date.now()} onread={() => notificationRead = true} onopen={() => notificationRead = true} />
    {#if notificationRead}<div><button class="btn" onclick={() => notificationRead = false}>Mark unread</button></div>{/if}
    <NotificationRow entry={{ id: 2, category: 'baro', title: "Baro Ki'Teer is here", body: baroNotice.expected_body, target: 'baro', created_at: Date.parse(baroNotice.now) / 1000, read: true, delivery: 'inbox_only', content: baroNotice.expected_content } as NotificationEntry} now={Date.now()} onread={() => {}} onopen={() => {}} />
  </section>
  {/if}
  {#if dataState === 'prompt'}<PromptBanner id="styleguide-prompt" session={samplePromptSession()} title="Help show what items really sell for"><p>Optional invitations use the neutral panel, never a toned edge, and say that ignoring them changes nothing. A long explanation wraps beside the actions and stacks above them in narrow windows.</p>{#snippet actions()}<button type="button" class="btn primary" onclick={() => (message = 'Sample prompt accepted.')}>Share sale prices</button><button type="button" class="btn">What is sent</button>{/snippet}</PromptBanner>{/if}
  {#if dataState === 'usage'}<UsageChart sample={usageSample} />{/if}
  {#if dataState === 'shimmer'}<section class="wrap tw ui-reading" aria-labelledby="shimmer-title"><div class="rail"><h3 id="shimmer-title">Field shimmer</h3></div><div class="bar"><span class="lbl">Idle</span><span class="shimmer-field grow" use:shimmer={{ mode: 'idle' }}><input class="input" type="text" aria-label="Idle shimmer sample" placeholder="At rest" /></span></div><div class="bar"><span class="lbl">Live</span><span class="shimmer-field grow" use:shimmer={{ mode: 'live', sparks: true }}><input class="input" type="text" aria-label="Live shimmer sample" placeholder="Waiting for a query" /></span></div><div class="bar"><span class="lbl">Pass</span><span class="shimmer-field grow" use:shimmer={{ mode: 'settled', pulse: shimmerPulse }}><input class="input" type="text" aria-label="Settled shimmer sample" placeholder="Focused from a shortcut" /></span><button class="btn xs" onclick={() => shimmerPulse++}>Run one pass</button></div><div class="body"><p>Idle is the quiet sweep a field shows at rest. Live is the full effect while a focused field waits for input, with sparks in the dark theme only. Pass runs once when a shortcut moves focus. Reduced motion shows the lit edges alone.</p></div></section>{/if}
  {#if dataState === 'app-icon'}<section class="wrap tw ui-reading" aria-labelledby="app-icon-title"><div class="rail"><h3 id="app-icon-title">App icon setting</h3></div><AppIconSetting appIcon={sampleAppIcon} /></section>{/if}
  {#if dataState === 'presence'}<section class="wrap tw ui-reading" aria-labelledby="presence-title"><div class="rail"><h3 id="presence-title">Trade presence</h3></div><p class="presence-marks"><span><PresenceMark kind="ingame" />Online in game</span><span><PresenceMark kind="online" />Online</span><span><PresenceMark kind="invisible" />Invisible</span><span><PresenceMark kind="warn" />Needs action</span></p><PresenceSettings presence={samplePresence} /></section>{/if}
  {#if dataState === 'sell'}<section class="ui-stack"><h2>Sortable analytical table</h2><p>Headers are sort buttons that announce their order; the Column guide explains every visible column.</p><ResultsTable results={sellSample} /></section>{/if}
  {#if dataState === 'updates'}<section class="ui-panel ui-stack"><h2>Changes across installed versions</h2><p>One plain-English summary combines skipped releases, with a version history beneath it.</p><button class="btn" onclick={() => notesRef?.open()}>Preview what’s new</button></section>{/if}
  {#if dataState === 'updates'}<UpdateNotes bind:this={notesRef} services={notesServices} />{/if}
  {#if dataState === 'token'}<section class="ui-panel ui-stack"><h2>Fallback sign-in</h2><p>When the in-app window cannot load warframe.market, the login dialog asks for the browser's session cookie and shows these steps.</p><div><button class="btn" onclick={() => tokenDialog.showModal()}>Preview token sign-in</button></div></section>{/if}
</main>

<dialog class="cryptobox" bind:this={tokenDialog} aria-labelledby="token-title">
  <form method="dialog">
    <header><h3 id="token-title">Log in to warframe.market</h3><p class="muted">Sample only. Nothing is sent or stored.</p></header>
    <label>Session token<input type="password" autocomplete="off" /></label>
    <WfmTokenGuide />
    <footer><button class="btn ghost" value="cancel" formnovalidate>Cancel</button><button class="btn primary" value="confirm">Save token</button></footer>
  </form>
</dialog>

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
  .presence-marks { display: flex; flex-wrap: wrap; gap: var(--s2) var(--s4); margin: 0; padding: var(--s3) var(--inset) 0; font-size: var(--text-caption); color: var(--muted); }
  .presence-marks span { display: inline-flex; align-items: center; gap: var(--s2); }
</style>
