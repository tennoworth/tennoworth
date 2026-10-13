<script lang="ts">
  // One row per kind of event across eight weeks, so overlaps read at a
  // glance: a rotation running under two Baro visits, a deadline inside it.
  import { eventEnd, type CalendarEvent, type EventKind } from '../../domain/calendar-events';
  import EventDetail from './EventDetail.svelte';
  import { DAY, dayStart, utcDay, weekStart } from './time';
  import { KIND_LABEL } from './kinds';

  let { events, now, kinds, surface }: { events: CalendarEvent[]; now: number; kinds: EventKind[]; surface: 'desktop' | 'hosted' } = $props();
  const DAYS = 56;
  const start = $derived(weekStart(now));
  const days = $derived(Array.from({ length: DAYS }, (_, i) => start + i * DAY));
  let selected = $state<string | null>(null);
  const shown = $derived(events.filter(e => eventEnd(e) >= start && e.from < start + DAYS * DAY));
  const chosen = $derived(shown.find(e => e.id === selected) ?? null);
  const months = $derived.by(() => {
    const spans: Array<{ label: string; first: number; span: number }> = [];
    days.forEach((d, i) => {
      const label = new Date(d).toLocaleDateString('en-GB', { month: 'long', timeZone: 'UTC' });
      if (spans.at(-1)?.label === label) spans.at(-1)!.span++;
      else spans.push({ label, first: i, span: 1 });
    });
    return spans;
  });
  function columns(event: CalendarEvent) {
    const first = Math.max(0, Math.floor((dayStart(Math.max(event.from, start)) - start) / DAY));
    const last = event.to === null ? first : Math.min(DAYS - 1, Math.floor((dayStart(event.to - 1) - start) / DAY));
    return { first, last, continued: event.from < start };
  }
  const nowOffset = $derived((now - start) / DAY);
</script>

<section class="lanes-panel" aria-label="Next eight weeks by kind">
  <div class="scroll">
    <div class="lanes" style:--days={DAYS} style:grid-template-rows="auto auto repeat({kinds.length}, auto)">
      <span class="corner" style:grid-row="1 / 3"></span>
      {#each months as m (m.label)}<span class="mon" style:grid-column="{m.first + 2} / span {m.span}">{m.label}</span>{/each}
      {#each days as d, i (d)}
        {@const wd = new Date(d).getUTCDay()}
        <span class="dn" class:weekend={wd === 0 || wd === 6} class:today={d === dayStart(now)} style:grid-column={i + 2} aria-hidden="true">{new Date(d).getUTCDate()}</span>
      {/each}
      {#each kinds as kind, r (kind)}
        <span class="lab" data-kind={kind} style:grid-row={r + 3}><i></i>{KIND_LABEL[kind]}</span>
        {#each days as d, i (d)}
          {@const wd = new Date(d).getUTCDay()}
          <span class="col" class:week-edge={wd === 1} class:weekend={wd === 0 || wd === 6} style:grid-row={r + 3} style:grid-column={i + 2}></span>
        {/each}
        {#each shown.filter(e => e.kind === kind) as event (event.id)}
          {@const c = columns(event)}
          <button type="button" class={event.to === null ? 'pin' : 'run'} class:estimate={event.estimate} data-kind={event.kind}
            style:grid-row={r + 3} style:grid-column="{c.first + 2} / {c.last + 3}" aria-pressed={selected === event.id}
            aria-label="{event.title}, {utcDay(event.from)}" title={event.title} onclick={() => { selected = selected === event.id ? null : event.id; }}>
            {#if event.to !== null}{c.continued ? '◂ ' : ''}{event.short}{/if}
          </button>
        {/each}
      {/each}
      <span class="now" style:grid-row="3 / {kinds.length + 3}" style:grid-column={Math.floor(nowOffset) + 2} style:margin-left="calc(var(--day) * {(nowOffset % 1).toFixed(2)})" aria-hidden="true"></span>
    </div>
  </div>
  <div class="chosen" aria-live="polite">
    {#if chosen}
      <strong>{chosen.title}{#if chosen.estimate}<span class="tag">est.</span>{/if}</strong>
      <EventDetail event={chosen} {surface} />
    {:else}
      <p class="hint">Choose a bar or a diamond for its dates and source. The ink line is now.</p>
    {/if}
  </div>
</section>

<style>
  .lanes-panel { border: 1px solid var(--border); background: var(--panel); min-width: 0; }
  .scroll { overflow-x: auto; }
  .lanes { --day: 1.75rem; display: grid; grid-template-columns: 9.5rem repeat(var(--days), var(--day)); min-width: max-content; position: relative; }
  .corner, .lab { position: sticky; left: 0; z-index: 3; background: var(--panel); border-right: 1px solid var(--border); }
  .corner { grid-column: 1; border-bottom: 1px solid var(--border); }
  .mon { grid-row: 1; padding: var(--s2) var(--s1) 0; font: 600 var(--text-caption)/1 var(--font-ui); letter-spacing: .14em; text-transform: uppercase; border-left: 1px solid var(--border); }
  .dn { grid-row: 2; text-align: center; font: 400 var(--text-caption)/1.7 var(--font-mono); color: var(--muted); border-bottom: 1px solid var(--border); }
  .dn.weekend, .col.weekend { background: var(--hatch); }
  .dn.today { background: var(--ink-bar); color: var(--on-ink); font-weight: 600; }
  .lab { grid-column: 1; display: flex; align-items: center; gap: var(--s2); padding: var(--s2) var(--s3); font: 600 var(--text-caption)/1.2 var(--font-ui); letter-spacing: .08em; text-transform: uppercase; border-top: 1px var(--rule) var(--hairline); }
  .lab i { width: 3px; align-self: stretch; background: var(--k); }
  .col { min-height: 2.75rem; border-left: 1px var(--rule) var(--hairline); border-top: 1px var(--rule) var(--hairline); }
  .col.week-edge { border-left: 1px solid var(--hairline); }
  .run { z-index: 2; align-self: center; min-width: 0; height: 1.75rem; margin: 0 1px; padding: 0 var(--s2); font: inherit; font-family: var(--font-body); letter-spacing: normal; text-transform: none; font-size: var(--text-caption); color: var(--fg); text-align: left; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; background: color-mix(in srgb, var(--k) 16%, var(--panel)); border: 1px solid var(--k); border-left-width: 3px; cursor: pointer; }
  .run.estimate { border-style: dashed; border-left-style: solid; background: transparent; }
  .pin { z-index: 2; align-self: center; justify-self: center; width: 1rem; height: 1rem; padding: 0; border: 0; background: var(--k); transform: rotate(45deg); cursor: pointer; }
  .pin.estimate { background: transparent; border: 1px dashed var(--k); }
  .run[aria-pressed="true"], .pin[aria-pressed="true"] { outline: 2px solid var(--fg); outline-offset: 1px; }
  .now { z-index: 2; border-left: 2px solid var(--fg); pointer-events: none; }
  .chosen { display: grid; gap: var(--s1); padding: var(--s3) var(--inset); border-top: 1px solid var(--border); }
  .chosen .tag { margin-left: var(--s2); }
  .hint { margin: 0; color: var(--muted); font-size: var(--text-caption); }
</style>
