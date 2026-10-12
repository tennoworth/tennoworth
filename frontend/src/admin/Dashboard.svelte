<script lang="ts">
  import { onMount } from 'svelte';
  import type { Publisher } from './api';
  import type { Change, Content, DashboardState, Message, PublicFeedCheck } from '../contracts/generated/publisher';
  import type { ThemeController } from '../ui/theme';
  import ThemeSwitcher from '../ui/ThemeSwitcher.svelte';
  import AnnouncementBanner from '../features/market-context/AnnouncementBanner.svelte';
  import { parseAnnouncements } from '../domain/announcements';
  import { calendarSuggestions, type Suggestion } from './suggestions';
  import limits from '../../../tests/fixtures/announcements/limits.json';

  let { publisher, theme }: { publisher: Publisher; theme: ThemeController } = $props();
  function initial(): Content {
    const start = new Date(); start.setSeconds(0, 0);
    const end = new Date(start.getTime() + 7 * 86400_000);
    return { title: '', body: '', source_url: '', starts_at: start.toISOString().replace('.000Z', 'Z'), ends_at: end.toISOString().replace('.000Z', 'Z') };
  }
  // Signing in again after Access expires reloads the page; this tab keeps unsaved work across it.
  const DRAFT_KEY = 'tennoworth-admin-draft-v1';
  type SavedDraft = { selected: string | null; form: Content; baseline: string; pending: Change | null; suggested?: string | null };
  function restore(): SavedDraft | null {
    try {
      const value = JSON.parse(sessionStorage.getItem(DRAFT_KEY) ?? 'null');
      const fields = ['title', 'body', 'source_url', 'starts_at', 'ends_at'] as const;
      return value && typeof value.baseline === 'string' && fields.every(field => typeof value.form?.[field] === 'string') ? value : null;
    } catch { return null; }
  }
  const restored = restore();
  let dashboardState = $state<DashboardState | null>(null);
  let selected = $state<string | null>(restored?.selected ?? null);
  const initialForm = initial();
  let form = $state<Content>(restored?.form ?? initialForm);
  let baseline = $state(restored?.baseline ?? JSON.stringify(initialForm));
  let busy = $state(false);
  let error = $state('');
  let receipt = $state(restored ? 'Restored your unsaved edits from before the page reloaded.' : '');
  let review = $state<'publish' | 'withdraw' | null>(null);
  let pending = $state<Change | null>(restored?.pending ?? null);
  let discard = $state<string | Suggestion | null>(null);
  let suggestions = $state<Suggestion[]>([]);
  let suggestionError = $state('');
  let suggested = $state<string | null>(restored?.suggested ?? null);
  // Per browser: a dismissal only hides a proposal, so losing it costs nothing.
  const DISMISSED_KEY = 'tennoworth-admin-dismissed-suggestions-v1';
  function readDismissed(): string[] {
    try { const value = JSON.parse(localStorage.getItem(DISMISSED_KEY) ?? '[]'); return Array.isArray(value) ? value.filter(key => typeof key === 'string') : []; }
    catch { return []; }
  }
  let dismissed = $state<string[]>(readDismissed());
  let feedCheck = $state<PublicFeedCheck | null>(null);
  let feedCheckError = $state('');
  let confirming = $state(false);
  let feedTimer: number | undefined;
  let checkSerial = 0;
  let now = $state(Date.now());
  const dirty = $derived(JSON.stringify(form) !== baseline);
  const record = $derived(dashboardState?.snapshot.messages.find(message => message.id === selected));
  const preview = $derived({ id: selected ?? 'draft-preview', ...form });
  const valid = $derived(!!parseAnnouncements({ version: 1, updated_at: new Date(now).toISOString().replace(/\.\d{3}Z$/, 'Z'), messages: [preview] }));
  const unpublished = $derived(!record?.published || JSON.stringify(record.draft) !== JSON.stringify(record.published));
  const exportPending = $derived(!!dashboardState && dashboardState.snapshot.feed_revision !== dashboardState.snapshot.exported_revision);
  const draftChanged = $derived(!!record && JSON.stringify(record.draft) !== baseline);
  const visibleSuggestions = $derived(suggestions.filter(suggestion => !dismissed.includes(suggestion.key)));
  const feedHost = $derived.by(() => {
    try { return feedCheck ? new URL(feedCheck.url).hostname : ''; } catch { return 'The local export'; }
  });
  const feedStatus = $derived.by((): { tone: string; text: string } => {
    if (feedCheckError) return { tone: 'warn', text: `Public feed not checked: ${feedCheckError}` };
    if (!feedCheck) return { tone: '', text: dashboardState ? 'Checking the public feed…' : 'Public feed unchecked' };
    const served = feedCheck.served_updated_at ? local(feedCheck.served_updated_at) : 'unknown';
    if (feedCheck.state === 'live') return { tone: 'good', text: `${feedHost} serves this feed · checked ${clock(feedCheck.checked_at)}` };
    if (feedCheck.state === 'behind') return { tone: 'warn', text: `${feedHost} still serves the feed from ${served}${confirming ? ' · rechecking' : ` · checked ${clock(feedCheck.checked_at)}`}` };
    if (feedCheck.state === 'different') return { tone: 'bad', text: `${feedHost} serves a newer feed (${served}) than this publisher published` };
    return { tone: 'warn', text: `Could not confirm ${feedHost}: ${feedCheck.error ?? 'no answer'}` };
  });
  $effect(() => {
    const draft = dirty || pending ? JSON.stringify({ selected, form, baseline, pending, suggested } satisfies SavedDraft) : null;
    try { if (draft) sessionStorage.setItem(DRAFT_KEY, draft); else sessionStorage.removeItem(DRAFT_KEY); }
    catch { /* The form still holds the edits; only a reload would lose them. */ }
  });

  async function refresh() {
    busy = true; error = '';
    if (review) receipt = 'Publication review closed. Review the latest saved draft before publishing.';
    review = null;
    try { dashboardState = await publisher.load(); void checkPublic(); }
    catch (cause) { error = cause instanceof Error ? cause.message : 'Could not load publication state.'; }
    finally { busy = false; }
  }
  async function checkPublic(): Promise<PublicFeedCheck | null> {
    const serial = ++checkSerial;
    try {
      const next = await publisher.publicFeed();
      if (serial === checkSerial) { feedCheck = next; feedCheckError = ''; }
      return next;
    } catch (cause) {
      if (serial === checkSerial) feedCheckError = cause instanceof Error ? cause.message : 'Could not check the public feed.';
      return null;
    }
  }
  // The export lands at once, but the site's cache may serve the old feed for a minute or so.
  function confirmPublication() {
    window.clearTimeout(feedTimer);
    feedCheck = null; confirming = true;
    let attempts = 0;
    const poll = async () => {
      const result = await checkPublic();
      attempts += 1;
      if (result?.state === 'live' || attempts >= 18) { confirming = false; return; }
      feedTimer = window.setTimeout(poll, 10_000);
    };
    void poll();
  }
  async function loadSuggestions() {
    try { suggestions = calendarSuggestions(await publisher.market(), Date.now()); suggestionError = ''; }
    catch (cause) { suggestionError = `Calendar suggestions are unavailable: ${cause instanceof Error ? cause.message : 'the market snapshot could not be read.'}`; }
  }
  function dismissSuggestion(key: string) {
    // Keys of events that have passed are dropped, so the list stays small.
    dismissed = [...dismissed.filter(entry => suggestions.some(suggestion => suggestion.key === entry)), key];
    try { localStorage.setItem(DISMISSED_KEY, JSON.stringify(dismissed)); } catch { /* Hidden until the page reloads. */ }
  }
  onMount(() => {
    void refresh();
    void loadSuggestions();
    const timer = window.setInterval(() => { now = Date.now(); }, 30_000);
    const leave = (event: BeforeUnloadEvent) => { if (dirty || pending) event.preventDefault(); };
    window.addEventListener('beforeunload', leave);
    return () => { window.clearInterval(timer); window.clearTimeout(feedTimer); window.removeEventListener('beforeunload', leave); };
  });

  function edit(target: string | Suggestion, confirmed = false) {
    if ((dirty || pending) && !confirmed) { discard = target; return; }
    const found = typeof target === 'string' ? dashboardState?.snapshot.messages.find(message => message.id === target) : undefined;
    const suggestion = typeof target === 'string' ? null : target;
    selected = found?.id ?? null;
    form = found ? { ...found.draft } : suggestion ? { ...suggestion.content } : initial();
    // A suggestion opens as unsaved edits, so leaving it asks first and Save is offered.
    baseline = suggestion ? JSON.stringify(initial()) : JSON.stringify(form);
    suggested = suggestion?.key ?? null;
    review = null; pending = null; error = ''; discard = null;
    receipt = suggestion ? 'Suggested draft opened. Check the wording and dates against the source, then save it.' : '';
  }
  async function submit(operation: Change['operation']) {
    if (!dashboardState || busy) return;
    const change: Change = pending ?? { request_id: crypto.randomUUID(), expected_revision: dashboardState.snapshot.revision, operation, id: selected, content: operation === 'save' ? { ...form } : null };
    pending = change; busy = true; error = ''; receipt = '';
    try {
      const next = await publisher.change(change);
      dashboardState = next;
      if (change.operation === 'save') {
        selected = change.id ?? next.message_id ?? null;
        baseline = JSON.stringify(change.content);
        if (suggested) { dismissSuggestion(suggested); suggested = null; }
      } else confirmPublication();
      receipt = change.operation === 'save' ? 'Draft saved. The public feed has not changed.'
        : next.snapshot.feed_revision === next.snapshot.exported_revision ? (change.operation === 'publish' ? 'Feed published. Running clients will pick it up on their next check.' : 'Withdrawal published. Existing popups cannot be recalled.')
        : 'Publication is pending: your revision was saved, but the public feed could not be replaced. The service will retry; refresh to check its status.';
      pending = null; review = null;
    } catch (cause) { error = cause instanceof Error ? cause.message : 'Could not save. Refresh before retrying.'; }
    finally { busy = false; }
  }
  function copyAsNew() {
    selected = null;
    form = { ...form };
    baseline = JSON.stringify(initial());
    review = null;
    receipt = 'Copied into a new draft. Save it, review its publication, then withdraw the original if it is wrong.';
  }
  function status(message: Message) {
    if (!message.published) return message.ever_published ? 'Withdrawn' : 'Draft';
    if (Date.parse(message.published.ends_at) <= now) return 'Expired';
    return Date.parse(message.published.starts_at) > now ? 'Scheduled' : 'Live';
  }
  function clock(value: string) { const date = new Date(value); return Number.isFinite(date.getTime()) ? date.toLocaleTimeString() : 'unknown'; }
  function local(value: string) { const date = new Date(value); return Number.isFinite(date.getTime()) ? date.toLocaleString() : 'Enter a valid UTC date'; }
  function dateValue(value: string) { return value.replace(/Z$/, '').slice(0, 19); }
  function setDate(field: 'starts_at' | 'ends_at', value: string) { form[field] = value ? value.length === 16 ? `${value}:00Z` : `${value}Z` : ''; review = null; }
</script>

<main class="admin-page">
  <header class="view-header">
    <h1>Announcements</h1>
    <p class="lede">Publish timely guidance to the site and desktop app.</p>
    <div class="vh-end"><button class="btn" onclick={refresh} disabled={busy}>Refresh status</button></div>
  </header>
  <section class="ui-summary-strip" aria-label="Publication status">
    <span>{dashboardState ? `Signed in: ${dashboardState.actor}` : 'Loading private publisher…'}</span>
    <span>{dashboardState ? `Revision ${dashboardState.snapshot.revision}` : 'Revision unavailable'}</span>
    <span>{exportPending ? 'Export pending — last public feed remains in use' : dashboardState ? `Feed updated ${local(dashboardState.snapshot.feed.updated_at)}` : 'Feed unavailable'}</span>
    <span class="feed-check" data-tone={feedStatus.tone || undefined} role="status">{feedStatus.text}</span>
  </section>
  {#if error}<div class="ui-notice" data-tone="bad" role="alert">{error}</div>{/if}
  {#if receipt}<div class="ui-notice" data-tone={exportPending ? 'warn' : 'good'} role="status">{receipt}</div>{/if}
  {#if draftChanged}<div class="ui-notice ui-stack" data-tone="warn"><p>This draft changed on the publisher. Your form has been preserved. Reload the saved draft or save your current wording before publication review.</p><button class="btn" disabled={busy} onclick={() => edit(selected!)}>Reload saved draft</button></div>{/if}
  {#if pending && !busy}
    <div class="ui-notice ui-stack" data-tone="warn">
      <p>The last request is unresolved. Refresh to inspect its status, or retry the same request safely.</p>
      <div class="ui-toolbar"><button class="btn" onclick={() => submit(pending!.operation)}>Retry request</button><button class="btn" onclick={() => { pending = null; review = null; }}>Keep edits and prepare a new request</button></div>
    </div>
  {/if}
  {#if discard}
    <div class="ui-notice ui-stack" data-tone="warn">
      <p>Keep working on these edits, or discard them to open another announcement?</p>
      <div class="ui-toolbar"><button class="btn" onclick={() => { discard = null; }}>Keep editing</button><button class="btn bad" onclick={() => edit(discard!, true)}>Discard edits</button></div>
    </div>
  {/if}
  {#if visibleSuggestions.length || suggestionError}
    <section class="ui-panel" aria-label="Suggested drafts">
      <header><h2>Suggested drafts</h2><span>From the game calendar · nothing publishes on its own</span></header>
      {#if suggestionError}<p class="hint">{suggestionError}</p>{/if}
      {#each visibleSuggestions as suggestion (suggestion.key)}
        <div class="suggestion">
          <div><strong id="suggestion-{suggestion.key}">{suggestion.headline}</strong><small>{suggestion.detail}</small></div>
          <div class="ui-toolbar">
            <button class="btn" disabled={busy} aria-describedby="suggestion-{suggestion.key}" onclick={() => edit(suggestion)}>Open as draft</button>
            <button class="btn ghost" aria-describedby="suggestion-{suggestion.key}" onclick={() => dismissSuggestion(suggestion.key)}>Dismiss</button>
          </div>
        </div>
      {/each}
    </section>
  {/if}
  <div class="admin-workspace">
    <section class="ui-panel announcements-list">
      <header><h2>Messages</h2><button class="btn" onclick={() => edit('new')} disabled={busy}>New announcement</button></header>
      {#if dashboardState && !dashboardState.snapshot.messages.length}<p>No announcements yet. Create a draft to get started.</p>{/if}
      {#each dashboardState?.snapshot.messages ?? [] as message (message.id)}
        <button class="message-choice" aria-pressed={selected === message.id} onclick={() => edit(message.id)} disabled={busy}>
          <strong>{message.draft.title}</strong><span>{status(message)}{message.published && JSON.stringify(message.draft) !== JSON.stringify(message.published) ? ' · unpublished edits' : ''}</span>
        </button>
      {/each}
    </section>
    <section class="ui-panel editor">
      <header><h2>{selected ? 'Edit announcement' : 'New announcement'}</h2><span>{dirty ? 'Unsaved edits' : selected ? 'Saved draft' : 'Draft'}</span></header>
      <form onsubmit={(event) => { event.preventDefault(); void submit('save'); }}>
        <fieldset disabled={busy || !!pending}>
          <label class="ui-field">Title<input bind:value={form.title} maxlength={limits.max_title * 2} oninput={() => { review = null; }} /></label>
          <label class="ui-field"><span id="message-label">Message</span><textarea aria-labelledby="message-label" aria-describedby="message-help" bind:value={form.body} rows="7" maxlength={limits.max_body * 2} oninput={() => { review = null; }}></textarea><small id="message-help">State the change, exact deadline, consequence and useful next action. Plain text only.</small></label>
          <label class="ui-field"><span id="source-label">Source URL</span><input aria-labelledby="source-label" aria-describedby="source-help" type="url" bind:value={form.source_url} oninput={() => { review = null; }} /><small id="source-help">Use a verified source on an approved host.</small></label>
          <div class="dates">
            <label class="ui-field">Starts at (UTC)<input type="datetime-local" step="1" value={dateValue(form.starts_at)} oninput={(event) => setDate('starts_at', event.currentTarget.value)} /><small>Your local time: {local(form.starts_at)}</small></label>
            <label class="ui-field">Ends at (UTC)<input type="datetime-local" step="1" value={dateValue(form.ends_at)} oninput={(event) => setDate('ends_at', event.currentTarget.value)} /><small>Your local time: {local(form.ends_at)}. Maximum 31 days.</small></label>
          </div>
          <div class="ui-toolbar">
            <button class="btn primary" type="submit" disabled={!dashboardState || !valid}>Save draft</button>
            <button class="btn" type="button" disabled={!record || dirty || draftChanged || !valid || !unpublished || Date.parse(form.ends_at) <= now} onclick={() => { review = 'publish'; }}>Review publication</button>
            {#if record?.published}<button class="btn bad" type="button" disabled={dirty} onclick={() => { review = 'withdraw'; }}>Review withdrawal</button>{/if}
          </div>
          {#if !valid}<p class="hint">Complete the title, message, approved source and valid start/end dates before saving.</p>{/if}
        </fieldset>
      </form>
      {#if review}
        <section class="ui-notice ui-stack" data-tone="warn" aria-label="Publication review">
          <strong>{review === 'publish' ? 'Ready to publish this saved draft?' : 'Withdraw this announcement?'}</strong>
          {#if review === 'publish'}
            <p>{record?.ever_published ? 'This ID has already been published. This edit will not repeat its desktop notification.' : 'This is a new announcement. Eligible desktop clients may receive one notification.'}</p>
            {#if record?.ever_published}<p>People who dismissed it or already received its popup will not see this edit. For a correction they must see, publish it as a new announcement.</p>{/if}
            <p>Starts {local(form.starts_at)} · Ends {local(form.ends_at)}. Running clients check every 15 minutes.</p>
          {:else}<p>The banner leaves after clients refresh. Offline clients may keep it until expiry; popups already sent cannot be recalled.</p>{/if}
          <div class="ui-toolbar"><button class="btn primary" disabled={busy} onclick={() => submit(review!)}>{review === 'publish' ? 'Publish announcement' : 'Withdraw announcement'}</button><button class="btn" disabled={busy} onclick={() => { review = null; }}>Cancel review</button>{#if review === 'publish' && record?.ever_published}<button class="btn" disabled={busy} onclick={copyAsNew}>Copy into a new announcement</button>{/if}</div>
        </section>
      {/if}
    </section>
  </div>
  <section class="ui-panel">
    <header><h2>Preview</h2><span>{Date.parse(form.starts_at) > now ? 'Scheduled — hidden until its start' : Date.parse(form.ends_at) <= now ? 'Expired — hidden from clients' : 'Active during its time window'}</span></header>
    {#if valid}<AnnouncementBanner message={preview} />{:else}<p>Complete the draft to preview the banner.</p>{/if}
    <details><summary>Desktop notification preview</summary><div class="notification-preview"><strong>{form.title || 'Announcement title'}</strong><p>{form.body || 'Announcement message'}</p><small>Uses the user’s popup preferences. The operating system controls its appearance and may shorten the text.</small></div></details>
  </section>
  <section class="ui-panel">
    <header><h2>Revision history</h2><span>Latest 100 changes</span></header>
    {#if dashboardState && !dashboardState.snapshot.history.length}<p>No changes recorded yet.</p>{/if}
    {#each dashboardState?.snapshot.history ?? [] as entry (entry.revision)}
      <details class="history-entry"><summary>Revision {entry.revision} · {entry.operation} · {local(entry.at)}</summary><p>{entry.actor}</p>{#if entry.content}<strong>{entry.content.title}</strong><p class="prose">{entry.content.body}</p><p>Starts {entry.content.starts_at} · Ends {entry.content.ends_at}</p><a href={entry.content.source_url} target="_blank" rel="noopener noreferrer">Verified source ↗</a>{/if}</details>
    {/each}
  </section>
  <footer><details><summary>Appearance</summary><ThemeSwitcher {theme} /></details><p>Private publishing · public feed only</p></footer>
</main>

<style>
  .admin-page { width: min(100%, 76rem); margin: 0 auto; padding: var(--s4); display: grid; gap: var(--s4); }
  h1 { margin: 0; font-size: var(--text-heading); }
  .admin-workspace { display: grid; grid-template-columns: minmax(14rem, 1fr) minmax(0, 2fr); gap: var(--s4); align-items: start; }
  .ui-panel > header { margin: calc(-1 * var(--s3)) calc(-1 * var(--inset)) 0; padding: var(--s3) var(--inset); background: var(--rail-bg); color: var(--rail-fg); border-bottom: 2px solid var(--rail-edge); }
  .ui-panel > header button { border-color: var(--rail-fg); color: var(--rail-fg); }
  .ui-panel > header button:disabled { background: transparent; color: var(--rail-muted); border: 1px dotted var(--rail-muted); }
  .ui-panel > header span { color: var(--rail-muted); }
  fieldset { border: 0; padding: 0; margin: 0; min-width: 0; display: grid; gap: var(--s3); }
  .dates { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: var(--s3); }
  .message-choice { display: grid; gap: var(--s1); text-align: left; border: 1px solid var(--border); background: var(--panel-2); color: var(--fg); padding: var(--s3); cursor: pointer; min-width: 0; overflow-wrap: anywhere; }
  .message-choice[aria-pressed="true"] { background: var(--ink-bar); color: var(--on-ink); }
  .message-choice span, .hint { font-size: var(--text-caption); }
  .hint, footer { color: var(--muted); }
  .feed-check[data-tone="good"] { color: var(--good); }
  .feed-check[data-tone="warn"] { color: var(--warn); }
  .feed-check[data-tone="bad"] { color: var(--bad); }
  .suggestion { display: grid; grid-template-columns: minmax(0, 1fr) auto; gap: var(--s2) var(--s3); align-items: center; padding-top: var(--s2); border-top: 1px var(--rule) var(--border); overflow-wrap: anywhere; }
  .suggestion:first-of-type { border-top: 0; padding-top: 0; }
  .suggestion small { display: block; color: var(--muted); font-size: var(--text-caption); }
  .notification-preview { padding: var(--s3); border: 1px solid var(--border); margin-top: var(--s2); }
  .prose, .notification-preview p { white-space: pre-line; overflow-wrap: anywhere; }
  .history-entry { border-bottom: 1px var(--rule) var(--border); padding-bottom: var(--s2); overflow-wrap: anywhere; }
  footer { display: flex; flex-wrap: wrap; justify-content: space-between; gap: var(--s3); }
  @media (max-width: 760px) { .admin-workspace, .dates, .suggestion { grid-template-columns: minmax(0, 1fr); } }
</style>
