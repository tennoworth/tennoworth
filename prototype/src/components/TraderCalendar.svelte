<script lang="ts">
  // Dated price shocks, soonest first.
  //
  // An unvaulting is the most expensive surprise in prime trading and it is
  // announced days ahead in worldState — the app just never read it. Rows that
  // touch something the user holds are marked; rows whose reach we cannot
  // determine say so rather than reading as "this doesn't affect you".
  import { buildCalendar, type CalendarItem } from '../lib/calendar-feed';
  import { glyphFor } from '../lib/glyphs';
  import type { Market, OwnedRecord } from '../lib/types';
  import Glyph from './Glyph.svelte';

  let {
    market,
    owned = null,
    now = Date.now(),
  }: {
    market: Market | null;
    owned?: Map<string, OwnedRecord> | null;
    /** Injectable so the row rendering is testable and stable. */
    now?: number;
  } = $props();

  let items = $derived(buildCalendar(market, owned, now));

  const GLYPH: Record<CalendarItem['kind'], Parameters<typeof glyphFor>[0]> = {
    baro: 'ducat',
    vault: 'relic',
    deal: 'credit',
    event: 'unknown',
  };

  /** "in 3d", "in 4h", "18h left" — relative, because every one of these is a
   *  deadline and an absolute date makes the reader do the arithmetic. */
  function when(item: CalendarItem): string {
    const start = Date.parse(item.at);
    if (start > now) {
      const h = Math.round((start - now) / 3_600_000);
      return h >= 24 ? `in ${Math.round(h / 24)}d` : `in ${h}h`;
    }
    const end = item.until ? Date.parse(item.until) : start;
    const h = Math.max(0, Math.round((end - now) / 3_600_000));
    return h >= 24 ? `${Math.round(h / 24)}d left` : `${h}h left`;
  }
</script>

{#if items.length}
  <section class="cal">
    <h3>What's coming</h3>
    <ul>
      {#each items as item (item.id ?? item.kind + item.at + item.title)}
        <li class:hot={item.affects.length > 0 && (item.affectsKnown || item.reach === 'partial-hits')}>
          <span class="when">{when(item)}</span>
          <span class="what">
            <Glyph name={glyphFor(GLYPH[item.kind])} />
            {item.title}
            {#if item.detail}<span class="detail">· {item.detail}</span>{/if}
          </span>
          <span class="hits">
            {#if item.kind === 'event' && item.reach === 'scan'}
              scan to check
            {:else if item.kind === 'event' && item.reach === 'partial-hits'}
              affects at least {item.affects.length} you hold
            {:else if item.kind === 'event' && item.reach === 'hits'}
              affects {item.affects.length} you hold
            {:else if item.kind === 'event' && item.reach === 'none'}
              none you hold
            {:else if item.kind === 'event' && item.reach === 'unknown'}
              <span class="unknown" title="Some fixed rewards could not be matched to market items."
                >reach unknown</span
              >
            {:else if item.affectsKnown && item.affects.length}
              affects {item.affects.length} you hold
            {:else if !item.affectsKnown}
              <span class="unknown" title="No scanned inventory, so we can't say what this touches."
                >reach unknown</span
              >
            {/if}
            {#if item.stale}<span class="stale"> · reward data {item.dataAgeDays === undefined ? 'age unknown' : `${item.dataAgeDays}d old`}</span>{/if}
          </span>
        </li>
      {/each}
    </ul>
  </section>
{/if}

<style>
  .cal {
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
  }
  h3 {
    margin: 0;
  }
  ul {
    margin: 0;
    padding: 0;
    list-style: none;
    display: flex;
    flex-direction: column;
  }
  li {
    display: flex;
    gap: 0.75rem;
    align-items: baseline;
    padding: 0.3rem 0;
    border-bottom: 1px solid var(--hairline);
    font-size: 0.9rem;
  }
  li:last-child {
    border-bottom: 0;
  }
  li.hot .what {
    color: var(--fg);
  }
  .when {
    min-width: 5rem;
    color: var(--muted);
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
  .what {
    flex: 1 1 auto;
    display: flex;
    align-items: center;
    gap: 0.35rem;
    color: var(--muted);
  }
  .detail,
  .hits {
    color: var(--faint);
    font-size: 0.85em;
  }
  li.hot .hits {
    color: var(--warn);
  }
  .unknown {
    cursor: help;
  }
  .stale {
    color: var(--warn);
  }
  @media (max-width: 42rem) {
    li {
      display: grid;
      grid-template-columns: 4.5rem minmax(0, 1fr);
      column-gap: 0.5rem;
    }
    .what {
      min-width: 0;
      flex-wrap: wrap;
    }
    .hits {
      grid-column: 2;
    }
  }
</style>
