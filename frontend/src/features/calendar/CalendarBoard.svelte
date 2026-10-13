<script lang="ts">
  // What's next and how long until it, then tear-off tiles by week. The view
  // for a glance between missions.
  import { eventEnd, eventKey, type CalendarEvent } from '../../domain/calendar-events';
  import EventDetail from './EventDetail.svelte';
  import { DAY, countdown, utcDay, utcTime, weekStart } from './time';

  let { events, now, surface }: { events: CalendarEvent[]; now: number; surface: 'desktop' | 'hosted' } = $props();
  let open = $state<string | null>(null);

  // Only a confirmed date earns the countdown. An event under way counts down
  // to its end, which is when a deadline like a Ducat change bites.
  const next = $derived(events.filter(e => !e.estimate && eventKey(e, now) > now).sort((a, b) => eventKey(a, now) - eventKey(b, now))[0] ?? null);
  const nextAt = $derived(next ? eventKey(next, now) : 0);
  const columns = $derived.by(() => {
    const thisWeek = weekStart(now) + 7 * DAY;
    const bounds: Array<[string, number, number]> = [['This week', now, thisWeek], ['Next week', thisWeek, thisWeek + 7 * DAY], ['Later', thisWeek + 7 * DAY, Infinity]];
    return bounds.map(([name, from, to]) => ({
      name,
      events: events.filter(e => eventEnd(e) >= now && eventKey(e, now) >= from && eventKey(e, now) < to).sort((a, b) => eventKey(a, now) - eventKey(b, now)),
    }));
  });
</script>

<div class="board">
  <section class="ui-panel next" aria-label="Next up">
    <header><h3>Next up</h3>{#if next}<span>{next.from <= now ? 'Ends' : 'Starts'} {utcDay(nextAt)}, {utcTime(nextAt)} UTC</span>{/if}</header>
    {#if next}
      <div class="next-body" data-kind={next.kind}>
        <strong class="count"><span class="cap">{next.from <= now ? 'Ends in' : 'In'}</span>{countdown(nextAt - now)}</strong>
        <div class="next-what"><strong>{next.title}</strong><span>{next.detail}</span></div>
      </div>
    {:else}
      <p class="hint">Nothing confirmed is coming up in the market data yet.</p>
    {/if}
  </section>

  <div class="columns">
    {#each columns as column (column.name)}
      <section class="column" aria-label={column.name}>
        <h3 class="col-h">{column.name} <span>{column.events.length}</span></h3>
        {#if !column.events.length}<p class="hint">Nothing scheduled.</p>{/if}
        {#each column.events as event (event.id)}
          {@const ongoing = event.from <= now}
          {@const date = eventKey(event, now)}
          <div class="tile" data-kind={event.kind} class:estimate={event.estimate}>
            <button type="button" class="tile-head" aria-expanded={open === event.id} onclick={() => { open = open === event.id ? null : event.id; }}>
              <span class="date" aria-hidden="true"><span class="m">{ongoing ? 'until' : utcDay(date).split(' ')[2]}</span><b>{new Date(date).getUTCDate()}</b><span class="m">{utcDay(date).split(' ')[0]}</span></span>
              <span class="what">
                <strong>{event.title}{#if event.estimate}<span class="tag">est.</span>{/if}</strong>
                <span class="meta">{ongoing ? `on now · ends ${utcDay(date)} ${utcTime(date)} UTC` : `${utcDay(date)}, ${utcTime(date)} UTC`}</span>
                {#if surface === 'desktop' && event.affects?.length}<span class="held">You hold {event.affects.length} affected item{event.affects.length === 1 ? '' : 's'}</span>{/if}
              </span>
            </button>
            {#if open === event.id}<div class="tile-body"><EventDetail {event} {surface} /></div>{/if}
          </div>
        {/each}
      </section>
    {/each}
  </div>
</div>

<style>
  .board { display: grid; gap: var(--s4); }
  .next-body { display: flex; flex-wrap: wrap; align-items: center; gap: var(--s2) var(--s5); border-left: 3px solid var(--k); padding-left: var(--s3); }
  .ui-panel > header { display: flex; flex-wrap: wrap; justify-content: space-between; align-items: center; gap: var(--s2); margin: calc(-1 * var(--s3)) calc(-1 * var(--inset)) 0; padding: var(--s2) var(--inset); background: var(--rail-bg); color: var(--rail-fg); border-bottom: 2px solid var(--rail-edge); }
  .ui-panel > header h3 { margin: 0; font-size: var(--text-section); }
  .ui-panel > header span { color: var(--rail-muted); font-size: var(--text-caption); }
  .count { display: grid; gap: var(--s1); font: 600 var(--text-metric-lg)/1 var(--font-mono); white-space: nowrap; }
  .count .cap { font: 600 var(--text-caption)/1 var(--font-ui); letter-spacing: .12em; text-transform: uppercase; color: var(--muted); }
  .held { font-size: var(--text-caption); font-weight: 600; }
  .next-what { display: grid; gap: var(--s1); flex: 1 1 18rem; min-width: 0; }
  .next-what span { color: var(--muted); font-size: var(--text-control); }
  .columns { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: var(--s4); align-items: start; }
  @media (max-width: 900px) { .columns { grid-template-columns: minmax(0, 1fr); } }
  .column { display: grid; gap: var(--s2); min-width: 0; }
  .col-h { margin: 0; display: flex; justify-content: space-between; align-items: baseline; font: 600 var(--text-section)/1.2 var(--font-ui); letter-spacing: .12em; text-transform: uppercase; border-bottom: 2px solid var(--fg); padding-bottom: var(--s1); }
  .col-h span { font: 400 var(--text-caption)/1 var(--font-mono); color: var(--muted); letter-spacing: 0; }
  .tile { border: 1px solid var(--border); background: var(--panel); min-width: 0; }
  .tile.estimate { border-style: dashed; }
  .tile-head { display: grid; grid-template-columns: 3.4rem minmax(0, 1fr); width: 100%; padding: 0; border: 0; background: transparent; color: inherit; font: inherit; text-align: left; cursor: pointer; }
  .tile-head:hover { background: var(--hover); }
  .date { display: grid; justify-items: center; align-content: start; padding: var(--s2) 0; background: var(--panel-2); border-right: 1px solid var(--border); border-top: 3px solid var(--k); }
  .tile.estimate .date { border-top-style: dashed; }
  .date b { font: 500 var(--text-heading)/1.1 var(--font-ui); }
  .date .m { font: 600 var(--text-caption)/1 var(--font-ui); letter-spacing: .1em; text-transform: uppercase; color: var(--muted); }
  .what { display: grid; gap: var(--s1); padding: var(--s2) var(--s3); min-width: 0; }
  .what strong { font-size: var(--text-body); line-height: 1.3; overflow-wrap: anywhere; }
  .what .tag { margin-left: var(--s2); }
  .meta { font: 400 var(--text-caption)/1.4 var(--font-mono); color: var(--muted); }
  .tile-body { padding: var(--s2) var(--s3) var(--s3); border-top: 1px var(--rule) var(--hairline); }
  .hint { margin: 0; color: var(--muted); font-size: var(--text-caption); }
</style>
