<script lang="ts">
  import type { CalendarEvent } from '../../domain/calendar-events';
  import { utcDay, utcTime, localTime } from './time';

  let { event, surface }: { event: CalendarEvent; surface: 'desktop' | 'hosted' } = $props();
  const SOURCE = { worldstate: 'DE worldState', estimate: 'Estimate', notice: 'TennoWorth notice' } as const;
</script>

<div class="detail" data-kind={event.kind} class:estimate={event.estimate}>
  <p class="when">
    {#if event.to === null}{utcDay(event.from)}, {utcTime(event.from)} UTC <span>({localTime(event.from)} your time)</span>
    {:else}{utcDay(event.from)} {utcTime(event.from)} → {utcDay(event.to)} {utcTime(event.to)} UTC{/if}
  </p>
  <p>{event.detail}</p>
  {#if surface === 'desktop' && event.affects}
    <p class="affects">{event.affects.length ? `You hold ${event.affects.length} affected item${event.affects.length === 1 ? '' : 's'}.` : 'Nothing you hold is affected.'}</p>
  {/if}
  <p class="source">
    <span class="who">{SOURCE[event.source]}</span>
    {#if event.sourceUrl}<a href={event.sourceUrl} target="_blank" rel="noopener noreferrer">Source ↗</a>{/if}
    {#if event.estimate}<span>No advice until DE confirms it.</span>{/if}
  </p>
</div>

<style>
  .detail { display: grid; gap: var(--s1); font-size: var(--text-control); }
  .detail p { margin: 0; overflow-wrap: anywhere; }
  .when { font-family: var(--font-mono); font-size: var(--text-caption); color: var(--muted); }
  .when span { font-family: var(--font-body); }
  .affects { color: var(--fg); font-weight: 600; }
  .source { display: flex; flex-wrap: wrap; gap: var(--s1) var(--s2); align-items: center; font-size: var(--text-caption); color: var(--muted); }
  .who { font: 600 var(--text-caption)/1 var(--font-ui); letter-spacing: .08em; text-transform: uppercase; color: var(--fg); border: 1px solid var(--border); padding: var(--s1); }
</style>
