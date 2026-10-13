<script lang="ts">
  import { untrack } from 'svelte';
  // A wall-calendar month: multi-day events as thin bars, single moments as
  // timed lines, and the chosen day's events in a panel beside it.
  import { eventEnd, type CalendarEvent, type EventKind } from '../../domain/calendar-events';
  import EventDetail from './EventDetail.svelte';
  import { DAY, dayStart, utcDay, utcTime, weekStart } from './time';

  let { events, now, surface }: { events: CalendarEvent[]; now: number; surface: 'desktop' | 'hosted' } = $props();
  const LANES = 4;
  const PRIORITY: EventKind[] = ['notice', 'official', 'baro', 'resurgence', 'vault', 'event', 'deal'];
  // Opens on the current month; navigating away is the reader's choice from then on.
  let month = $state(untrack(() => Date.UTC(new Date(now).getUTCFullYear(), new Date(now).getUTCMonth(), 1)));
  let day = $state(untrack(() => dayStart(now)));
  const today = $derived(dayStart(now));

  function shift(by: number) {
    const d = new Date(month);
    month = Date.UTC(d.getUTCFullYear(), d.getUTCMonth() + by, 1);
  }
  function goToday() {
    month = Date.UTC(new Date(now).getUTCFullYear(), new Date(now).getUTCMonth(), 1);
    day = today;
  }
  const label = $derived(new Date(month).toLocaleDateString('en-GB', { month: 'long', year: 'numeric', timeZone: 'UTC' }));
  const weeks = $derived.by(() => {
    const m = new Date(month).getUTCMonth();
    const last = Date.UTC(new Date(month).getUTCFullYear(), m + 1, 0);
    const rows = [];
    for (let start = weekStart(month); start <= last; start += 7 * DAY) {
      const end = start + 7 * DAY;
      const lanes: number[] = [];
      const bars = [];
      const more = Array(7).fill(0);
      const inWeek = events.filter(e => e.from < end && eventEnd(e) >= start)
        // When a week is crowded, what a player acts on gets a lane before month-long events.
        .sort((a, b) => PRIORITY.indexOf(a.kind) - PRIORITY.indexOf(b.kind) || a.from - b.from);
      for (const event of inWeek) {
        const first = Math.max(0, Math.floor((dayStart(Math.max(event.from, start)) - start) / DAY));
        const lastCol = event.to === null ? first : Math.min(6, Math.floor((dayStart(Math.min(event.to - 1, end - 1)) - start) / DAY));
        let lane = lanes.findIndex(taken => taken < first);
        if (lane === -1) { lane = lanes.length; lanes.push(lastCol); } else lanes[lane] = lastCol;
        if (lane >= LANES) { for (let c = first; c <= lastCol; c++) more[c]++; continue; }
        bars.push({ event, first, lastCol, lane, continued: event.from < start });
      }
      rows.push({ start, days: Array.from({ length: 7 }, (_, i) => start + i * DAY), bars, more, month: m });
    }
    return rows;
  });
  const onDay = $derived(events.filter(e => dayStart(e.from) <= day && dayStart(eventEnd(e)) >= day));
  const nextUp = $derived(onDay.length ? [] : events.filter(e => e.from >= day + DAY).slice(0, 3));
  const count = (d: number) => events.filter(e => dayStart(e.from) <= d && dayStart(eventEnd(e)) >= d).length;
</script>

<div class="month-view">
  <section class="month" aria-label={label}>
    <div class="mhead">
      <h3>{label}</h3>
      <div class="ui-toolbar">
        <button type="button" class="btn xs" aria-label="Previous month" onclick={() => shift(-1)}>‹</button>
        <button type="button" class="btn xs" onclick={goToday}>Today</button>
        <button type="button" class="btn xs" aria-label="Next month" onclick={() => shift(1)}>›</button>
      </div>
    </div>
    <div class="dow" aria-hidden="true"><span>Mon</span><span>Tue</span><span>Wed</span><span>Thu</span><span>Fri</span><span>Sat</span><span>Sun</span></div>
    {#each weeks as week (week.start)}
      <div class="week">
        {#each week.days as d, i (d)}
          {@const n = count(d)}
          <button type="button" class="cell" class:out={new Date(d).getUTCMonth() !== week.month} style:grid-column={i + 1}
            aria-pressed={d === day} aria-label="{utcDay(d, true)}, {n ? `${n} event${n === 1 ? '' : 's'}` : 'nothing scheduled'}" onclick={() => { day = d; }}></button>
          <span class="num" class:out={new Date(d).getUTCMonth() !== week.month} class:today={d === today} style:grid-column={i + 1} aria-hidden="true"><b>{new Date(d).getUTCDate()}</b></span>
        {/each}
        {#each week.bars as bar (bar.event.id)}
          <button type="button" class={bar.event.to === null ? 'ev-moment' : 'ev-run'} class:estimate={bar.event.estimate} data-kind={bar.event.kind}
            style:grid-column="{bar.first + 1} / {bar.lastCol + 2}" style:grid-row={bar.lane + 2} title={bar.event.title} aria-label={bar.event.title}
            onclick={() => { day = Math.max(dayStart(bar.event.from), week.start); }}>
            {#if bar.event.to === null}<span class="tm">{utcTime(bar.event.from)}</span>{/if}<span class="ev-text">{bar.continued ? '◂ ' : ''}{bar.event.short}</span>
          </button>
        {/each}
        {#each week.more as n, i (i)}
          {#if n}<button type="button" class="more" style:grid-column={i + 1} style:grid-row={LANES + 2} onclick={() => { day = week.start + i * DAY; }}>+{n} more</button>{/if}
        {/each}
      </div>
    {/each}
  </section>

  <section class="ui-panel day" aria-live="polite" aria-label="Selected day">
    <header><h3>{day === today ? 'Today' : 'Day'}</h3><span>Times in UTC</span></header>
    <p class="big"><b>{new Date(day).getUTCDate()}</b><span>{utcDay(day, true)}</span></p>
    {#each onDay as event (event.id)}
      <div class="entry" data-kind={event.kind} class:estimate={event.estimate}>
        <strong>{event.title}{#if event.estimate}<span class="tag">est.</span>{/if}</strong>
        <EventDetail {event} {surface} />
      </div>
    {:else}
      <p class="hint">Nothing scheduled.</p>
      {#if nextUp.length}<p class="hint">Coming next:</p>{/if}
      {#each nextUp as event (event.id)}
        <button type="button" class="btn ghost xs next" onclick={() => { day = dayStart(event.from); month = Date.UTC(new Date(event.from).getUTCFullYear(), new Date(event.from).getUTCMonth(), 1); }}>{utcDay(event.from)} · {event.short}</button>
      {/each}
    {/each}
  </section>
</div>

<style>
  .month-view { display: grid; grid-template-columns: minmax(0, 1fr) 19rem; gap: var(--s4); align-items: start; }
  @media (max-width: 1100px) { .month-view { grid-template-columns: minmax(0, 1fr); } }
  .month { border: 1px solid var(--border); background: var(--panel); min-width: 0; }
  .mhead { display: flex; flex-wrap: wrap; align-items: center; gap: var(--s2) var(--s3); padding: var(--s2) var(--inset); background: var(--rail-bg); color: var(--rail-fg); border-bottom: 2px solid var(--rail-edge); }
  .mhead h3 { margin: 0 auto 0 0; font: 600 var(--text-section)/1.2 var(--font-ui); letter-spacing: .12em; text-transform: uppercase; }
  .mhead .btn { background: transparent; border-color: var(--rail-fg); color: var(--rail-fg); }
  .dow { display: grid; grid-template-columns: repeat(7, minmax(0, 1fr)); background: var(--panel-2); border-bottom: 1px solid var(--border); }
  .dow span { padding: 0 var(--cell); height: var(--head); display: flex; align-items: center; font: 600 var(--text-caption)/1 var(--font-ui); letter-spacing: .08em; text-transform: uppercase; color: var(--muted); }
  .week { display: grid; grid-template-columns: repeat(7, minmax(0, 1fr)); row-gap: var(--s1); padding-bottom: var(--s2); min-height: 6.5rem; border-top: 1px solid var(--hairline); }
  .week:first-of-type { border-top: 0; }
  .cell { grid-row: 1 / -1; min-height: 100%; border: 0; border-left: 1px var(--rule) var(--hairline); background: transparent; padding: 0; cursor: pointer; }
  .cell:nth-child(1) { border-left: 0; }
  .cell:hover { background: var(--hover); }
  .cell.out { background: repeating-linear-gradient(135deg, var(--hatch) 0 2px, transparent 2px 7px); }
  .cell[aria-pressed="true"] { box-shadow: inset 0 0 0 2px var(--fg); }
  .num { grid-row: 1; padding: var(--s1) var(--cell) 0; font: 500 var(--text-section)/1.2 var(--font-ui); pointer-events: none; z-index: 1; }
  .num.out { color: var(--muted); }
  .num.today b { background: var(--ink-bar); color: var(--on-ink); padding: 0 var(--s1); }
  .ev-run, .ev-moment, .more { z-index: 2; min-width: 0; margin: 0 var(--s1); font: inherit; font-family: var(--font-body); letter-spacing: normal; text-transform: none; font-size: var(--text-caption); line-height: 1.35; text-align: left; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; cursor: pointer; color: var(--fg); }
  .ev-run { padding: 0 var(--s2); border: 0; border-top: 2px solid var(--k); background: color-mix(in srgb, var(--k) 12%, transparent); }
  .ev-run.estimate { border-top-style: dashed; background: transparent; }
  .ev-moment { padding: 0; border: 0; background: transparent; }
  .ev-moment::before { content: ""; display: inline-block; width: .45rem; height: .45rem; margin-right: var(--s1); background: var(--k); vertical-align: .05rem; }
  .ev-moment.estimate::before { background: transparent; border: 1px dashed var(--k); }
  .tm { font-family: var(--font-mono); color: var(--muted); margin-right: var(--s1); }
  .more { border: 0; background: transparent; color: var(--muted); padding: 0 var(--s2); }
  .day > header { display: flex; flex-wrap: wrap; justify-content: space-between; align-items: center; gap: var(--s2); margin: calc(-1 * var(--s3)) calc(-1 * var(--inset)) 0; padding: var(--s2) var(--inset); background: var(--rail-bg); color: var(--rail-fg); border-bottom: 2px solid var(--rail-edge); }
  .day > header h3 { margin: 0; font-size: var(--text-section); }
  .day > header span { color: var(--rail-muted); font-size: var(--text-caption); }
  .day .big { margin: 0; display: flex; align-items: baseline; gap: var(--s3); border-bottom: 2px solid var(--fg); padding-bottom: var(--s2); }
  .day .big b { font: 500 2.5rem/0.9 var(--font-ui); }
  .day .big span { font: 600 var(--text-section)/1.2 var(--font-ui); letter-spacing: .06em; }
  .entry { display: grid; gap: var(--s1); padding-left: var(--s3); border-left: 3px solid var(--k); }
  .entry.estimate { border-left-style: dashed; }
  .entry .tag { margin-left: var(--s2); }
  .next { justify-content: flex-start; }
  .hint { margin: 0; color: var(--muted); font-size: var(--text-caption); }
  @media (max-width: 640px) {
    /* A phone shows the shape of the month; the day panel carries the words. */
    .week { min-height: 3.5rem; }
    .ev-run, .ev-moment { height: .35rem; padding: 0; }
    .ev-moment::before { width: .35rem; height: .35rem; margin: 0; vertical-align: top; }
    .ev-text, .tm, .more { display: none; }
  }
</style>
