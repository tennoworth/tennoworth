<script lang="ts">
  // Official DE dates for the player calendar: devstreams, updates, Prime
  // Access. They go straight into the public feed after an explicit review,
  // with no draft stage, because a date either is announced or is not.
  import type { Publisher } from './api';
  import type { DashboardState, EventChange, EventContent } from '../contracts/generated/publisher';
  import type { OfficialEvent } from '../contracts/generated/desktop';
  import { parseAnnouncements } from '../domain/announcements';
  import limits from '../../../tests/fixtures/announcements/limits.json';

  let { publisher, dashboardState, onstate }: {
    publisher: Publisher;
    dashboardState: DashboardState | null;
    onstate: (next: DashboardState) => void;
  } = $props();

  const KIND_LABEL: Record<string, string> = {
    update: 'Update', devstream: 'Devstream', 'prime-access': 'Prime Access', drops: 'Twitch drops',
    nightwave: 'Nightwave', ducats: 'Ducat change', event: 'In-game event', other: 'Other',
  };
  function blank(): EventContent {
    const start = new Date(); start.setUTCMinutes(0, 0, 0); start.setUTCDate(start.getUTCDate() + 1);
    return { kind: 'devstream', title: '', starts_at: start.toISOString().replace('.000Z', 'Z'), ends_at: null, source_url: '', note: null };
  }
  let form = $state<EventContent>(blank());
  let editing = $state<string | null>(null);
  let review = $state<'publish' | 'remove' | null>(null);
  let removing = $state<OfficialEvent | null>(null);
  let pending = $state<EventChange | null>(null);
  let busy = $state(false);
  let error = $state('');
  let receipt = $state('');

  const events = $derived(dashboardState?.snapshot.feed.events ?? []);
  const candidate = $derived({ id: editing ?? 'event-preview', ...form, note: form.note?.trim() ? form.note : null });
  const valid = $derived(!!parseAnnouncements({ version: 1, updated_at: '2026-01-01T00:00:00Z', messages: [], events: [candidate] }));

  function dateValue(value: string | null) { return (value ?? '').replace(/Z$/, '').slice(0, 19); }
  function stamp(value: string) { return value ? (value.length === 16 ? `${value}:00Z` : `${value}Z`) : ''; }
  function utc(value: string) { const d = new Date(value); return Number.isFinite(d.getTime()) ? `${d.toLocaleString('en-GB', { timeZone: 'UTC', weekday: 'short', day: 'numeric', month: 'short', hour: '2-digit', minute: '2-digit' })} UTC` : 'Enter a valid UTC date'; }
  function local(value: string) { const d = new Date(value); return Number.isFinite(d.getTime()) ? d.toLocaleString() : ''; }

  function edit(event: OfficialEvent | null) {
    editing = event?.id ?? null;
    form = event ? { kind: event.kind, title: event.title, starts_at: event.starts_at, ends_at: event.ends_at ?? null, source_url: event.source_url, note: event.note ?? null } : blank();
    review = null; removing = null; pending = null; error = ''; receipt = '';
  }

  async function send(change: EventChange, done: string) {
    if (busy) return;
    pending = change; busy = true; error = '';
    try {
      onstate(await publisher.changeEvent(change));
      edit(null);
      receipt = done;
    } catch (cause) {
      error = cause instanceof Error ? cause.message : 'Could not publish. Refresh before retrying.';
    } finally { busy = false; }
  }
  function publish() {
    if (!dashboardState) return;
    void send(pending ?? { request_id: crypto.randomUUID(), expected_revision: dashboardState.snapshot.revision, operation: 'publish', id: editing, event: { ...form, note: form.note?.trim() ? form.note : null } },
      editing ? 'Event corrected. Calendars pick it up on their next feed check.' : 'Event published. Calendars pick it up on their next feed check.');
  }
  function remove() {
    if (!dashboardState || !removing) return;
    void send(pending ?? { request_id: crypto.randomUUID(), expected_revision: dashboardState.snapshot.revision, operation: 'remove', id: removing.id, event: null },
      'Event removed from the calendar.');
  }
</script>

<section class="ui-panel events" aria-label="Official dates">
  <header><h2>Official dates</h2><span>DE announcements for the player calendar</span></header>
  {#if error}<div class="ui-notice" data-tone="bad" role="alert">{error}{#if pending}<div class="ui-toolbar"><button class="btn" disabled={busy} onclick={() => (pending!.operation === 'publish' ? publish() : remove())}>Retry the same request</button></div>{/if}</div>{/if}
  {#if receipt}<div class="ui-notice" data-tone="good" role="status">{receipt}</div>{/if}

  {#if !events.length}<p class="hint">No official dates published. Add a devstream, update or Prime Access date with the post that announced it.</p>{/if}
  {#each events as event (event.id)}
    <div class="row">
      <div class="what"><strong>{event.title}</strong><span>{KIND_LABEL[event.kind] ?? event.kind} · {utc(event.starts_at)}{event.ends_at ? ` → ${utc(event.ends_at)}` : ''}</span></div>
      <div class="ui-toolbar">
        <button class="btn" disabled={busy} onclick={() => edit(event)}>Edit</button>
        <button class="btn bad" disabled={busy} onclick={() => { removing = event; review = 'remove'; pending = null; error = ''; receipt = ''; }}>Remove</button>
      </div>
    </div>
  {/each}

  {#if review === 'remove' && removing}
    <div class="ui-notice ui-stack" data-tone="warn" aria-label="Removal review">
      <strong>Remove “{removing.title}” from the calendar?</strong>
      <p>It leaves the site after its next feed check and the desktop app within 15 minutes. Use Edit instead if only the date or wording changed.</p>
      <div class="ui-toolbar"><button class="btn primary" disabled={busy} onclick={remove}>Remove event</button><button class="btn" disabled={busy} onclick={() => { review = null; removing = null; }}>Keep it</button></div>
    </div>
  {/if}

  <form onsubmit={(e) => { e.preventDefault(); if (valid) review = 'publish'; }}>
    <h3>{editing ? 'Correct this date' : 'Add an official date'}</h3>
    <fieldset disabled={busy}>
      <div class="pair">
        <label class="ui-field">Event kind<select bind:value={form.kind} onchange={() => { review = null; }}>{#each limits.event_kinds as kind (kind)}<option value={kind}>{KIND_LABEL[kind] ?? kind}</option>{/each}</select></label>
        <label class="ui-field">Event title<input bind:value={form.title} maxlength={limits.max_title} oninput={() => { review = null; }} /></label>
      </div>
      <div class="pair">
        <label class="ui-field">Event starts at (UTC)<input type="datetime-local" step="60" value={dateValue(form.starts_at)} oninput={(e) => { form.starts_at = stamp(e.currentTarget.value); review = null; }} /><small>Your local time: {local(form.starts_at)}</small></label>
        <label class="ui-field">Event ends at (UTC, optional)<input type="datetime-local" step="60" value={dateValue(form.ends_at)} oninput={(e) => { form.ends_at = e.currentTarget.value ? stamp(e.currentTarget.value) : null; review = null; }} /><small>Leave empty for a single moment such as a stream start. At most 120 days.</small></label>
      </div>
      <label class="ui-field">Link to DE's announcement<input type="url" bind:value={form.source_url} oninput={() => { review = null; }} /><small>The DE post that announced it, on an approved host.</small></label>
      <label class="ui-field">Event note (optional)<textarea rows="2" maxlength={limits.max_event_note} value={form.note ?? ''} oninput={(e) => { form.note = e.currentTarget.value || null; review = null; }}></textarea><small>Shown instead of the default description, up to {limits.max_event_note} characters.</small></label>
      <div class="ui-toolbar">
        <button class="btn primary" type="submit" disabled={!valid || !dashboardState}>Review {editing ? 'correction' : 'event'}</button>
        {#if editing}<button class="btn" type="button" onclick={() => edit(null)}>Cancel editing</button>{/if}
      </div>
      {#if !valid}<p class="hint">Fill in a title, an approved source and valid dates to continue.</p>{/if}
    </fieldset>
  </form>

  {#if review === 'publish'}
    <div class="ui-notice ui-stack" data-tone="warn" aria-label="Publication review">
      <strong>{editing ? 'Publish this correction?' : 'Publish this date to the player calendar?'}</strong>
      <p><b>{form.title}</b> · {KIND_LABEL[form.kind] ?? form.kind} · {utc(form.starts_at)}{form.ends_at ? ` → ${utc(form.ends_at)}` : ''}</p>
      <p>Source: {form.source_url}</p>
      <p>It appears on tennoworth.app and in the desktop app's calendar. It does not raise a notification.</p>
      <div class="ui-toolbar"><button class="btn primary" disabled={busy} onclick={publish}>{editing ? 'Publish correction' : 'Publish event'}</button><button class="btn" disabled={busy} onclick={() => { review = null; }}>Back to editing</button></div>
    </div>
  {/if}
</section>

<style>
  .events > header { margin: calc(-1 * var(--s3)) calc(-1 * var(--inset)) 0; padding: var(--s3) var(--inset); background: var(--rail-bg); color: var(--rail-fg); border-bottom: 2px solid var(--rail-edge); display: flex; flex-wrap: wrap; justify-content: space-between; gap: var(--s2); align-items: center; }
  .events > header h2 { margin: 0; font-size: var(--text-section); }
  .events > header span { color: var(--rail-muted); font-size: var(--text-caption); }
  .row { display: flex; flex-wrap: wrap; justify-content: space-between; gap: var(--s2) var(--s3); align-items: center; padding-block: var(--s2); border-bottom: 1px var(--rule) var(--border); }
  .what { display: grid; gap: var(--s1); min-width: 0; overflow-wrap: anywhere; }
  .what span { color: var(--muted); font-size: var(--text-caption); }
  form h3 { margin: var(--s2) 0 0; font-size: var(--text-section); }
  fieldset { border: 0; padding: 0; margin: 0; min-width: 0; display: grid; gap: var(--s3); }
  .pair { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: var(--s3); }
  .hint { margin: 0; color: var(--muted); font-size: var(--text-caption); }
  .ui-notice p { margin: 0; overflow-wrap: anywhere; }
  @media (max-width: 760px) { .pair { grid-template-columns: minmax(0, 1fr); } }
</style>
