<script lang="ts">
  import { roving } from '../../ui/keyboard-actions';
  // The dated things that move Warframe prices, in three views of one list:
  // Board for "what's next", Month for the familiar grid, Lanes for overlaps.
  import { onMount, untrack } from 'svelte';
  import { calendarEvents, type EventKind } from '../../domain/calendar-events';
  import { parseAnnouncements } from '../../domain/announcements';
  import type { Market, OwnedRecord } from '../../contracts/data';
  import type { AnnouncementFeed } from '../../contracts/generated/desktop';
  import type { SettingsStore } from '../../contracts/state-store';
  import CalendarBoard from './CalendarBoard.svelte';
  import CalendarMonth from './CalendarMonth.svelte';
  import CalendarLanes from './CalendarLanes.svelte';
  import { t } from '../../i18n/i18n.svelte';
  import { HIDDEN_BY_DEFAULT, KINDS, kindLabel } from './kinds';

  type View = 'board' | 'month' | 'lanes';
  let { market, owned = null, now, loadAnnouncements, store, surface, heading = 'h2' }: {
    market: Market | null;
    owned?: Map<string, OwnedRecord> | null;
    now: number;
    loadAnnouncements?: () => Promise<AnnouncementFeed | null>;
    store?: SettingsStore;
    surface: 'desktop' | 'hosted';
    heading?: 'h2' | 'h3';
  } = $props();

  const VIEWS: View[] = ['board', 'month', 'lanes'];
  function restore(): { view: View; hidden: EventKind[] } {
    try {
      const value = JSON.parse(store?.getSetting('calendar') ?? 'null');
      const view = VIEWS.includes(value?.view as View) ? value.view as View : 'board';
      const hidden = Array.isArray(value?.hidden) ? value.hidden.filter((k: unknown): k is EventKind => KINDS.includes(k as EventKind)) : HIDDEN_BY_DEFAULT;
      return { view, hidden };
    } catch { return { view: 'board', hidden: HIDDEN_BY_DEFAULT }; }
  }
  const saved = untrack(restore);
  let view = $state<View>(saved.view);
  let hidden = $state<EventKind[]>(saved.hidden);
  let feed = $state<AnnouncementFeed | null>(null);

  function persist() {
    void store?.setSetting('calendar', JSON.stringify({ view, hidden })).catch(() => { /* The choice still applies until reload. */ });
  }
  function choose(next: View) { view = next; persist(); }
  function toggle(kind: EventKind, on: boolean) {
    hidden = on ? hidden.filter(k => k !== kind) : [...hidden, kind];
    persist();
  }

  onMount(() => {
    let active = true;
    loadAnnouncements?.().then(value => { const parsed = parseAnnouncements(value); if (active && parsed) feed = parsed; })
      // Notices are one source among several; the rest of the calendar stands without them.
      .catch(() => {});
    return () => { active = false; };
  });

  const all = $derived(calendarEvents(market, feed, now, owned));
  const events = $derived(all.filter(e => !hidden.includes(e.kind)));
  const kindsShown = $derived(KINDS.filter(k => !hidden.includes(k)));
</script>

<section class="calendar" aria-labelledby="calendar-title">
  <div class="view-header">
    <svelte:element this={heading} id="calendar-title">{t('cal.title')}</svelte:element>
    <p class="lede">{t(surface === 'desktop' ? 'cal.ledeDesktop' : 'cal.ledeSite')}</p>
    <div class="vh-end">
      <div class="ui-segmented" role="group" aria-label={t('cal.viewChoice')} use:roving>
        {#each VIEWS as id (id)}<button type="button" aria-pressed={view === id} onclick={() => choose(id)}>{t(`cal.view.${id}`)}</button>{/each}
      </div>
    </div>
  </div>

  <fieldset class="kinds">
    <legend class="sr-only">{t('cal.show')}</legend>
    {#each KINDS as kind (kind)}
      <label class="kind" data-kind={kind}><input type="checkbox" checked={!hidden.includes(kind)} onchange={(e) => toggle(kind, e.currentTarget.checked)} /><span class="swatch" aria-hidden="true"></span>{kindLabel(kind)}</label>
    {/each}
    <span class="hint">{t('cal.dashed')}</span>
  </fieldset>

  {#if !market && !feed}
    <p class="ui-notice" role="status">{t('cal.loading')}</p>
  {:else if view === 'board'}
    <CalendarBoard {events} {now} {surface} />
  {:else if view === 'month'}
    <CalendarMonth {events} {now} {surface} />
  {:else}
    <CalendarLanes {events} {now} kinds={kindsShown} {surface} />
  {/if}
</section>

<style>
  .calendar { display: grid; gap: var(--s4); min-width: 0; }
  .kinds { display: flex; flex-wrap: wrap; align-items: center; gap: var(--s2); border: 0; margin: 0; padding: 0; min-width: 0; }
  .kind { display: inline-flex; align-items: center; gap: var(--s2); min-height: var(--ctl); padding: 0 var(--s3) 0 var(--s2); border: 1px solid var(--border); background: var(--panel); font-size: var(--text-caption); cursor: pointer; }
  .swatch { width: .9rem; height: .6rem; background: var(--panel-2); border: 1px solid var(--border); border-left: 3px solid var(--k); }
  .hint { color: var(--muted); font-size: var(--text-caption); }
  .sr-only { position: absolute; width: 1px; height: 1px; overflow: hidden; clip: rect(0 0 0 0); white-space: nowrap; }
</style>
