<script lang="ts">
  import { onMount, tick, untrack } from 'svelte';
  import type { Market, OwnedRecord } from '../../contracts/data';
  import { baroLocation, humanWindow } from '../../ui/format';
  import TraderCalendar from '../market-context/TraderCalendar.svelte';
  import { MAX_GOALS, MAX_GOAL_LENGTH, nextRoutinePeriod, type RoutineController, type RoutineCadence, type RoutineTask } from './controller.svelte';

  let {
    routine,
    market,
    owned,
    now = Date.now(),
  }: {
    routine: RoutineController;
    market: Market | null;
    owned: Map<string, OwnedRecord>;
    now?: number;
  } = $props();

  let baro = $derived(market?.baro ? { ...market.baro, location: baroLocation(market.baro.location) } : null);
  let baroWindow = $derived.by(() => {
    if (!baro) return null;
    const arrival = Date.parse(baro.activation);
    const expiry = Date.parse(baro.expiry);
    if (Number.isFinite(arrival) && Number.isFinite(expiry) && now >= arrival && now < expiry) {
      return { label: 'Baro leaves', duration: expiry - now };
    }
    if (Number.isFinite(arrival) && now < arrival) return { label: 'Baro arrives', duration: arrival - now };
    return null;
  });
  let completed = $derived(routine.completed);
  let progress = $derived(`${completed.size} of ${routine.tasks.length} complete`);

  const cadenceLabels: Record<RoutineCadence, string> = { daily: 'Daily', weekly: 'Weekly', monthly: 'Monthly' };

  function periodLabel(cadence: RoutineCadence): string {
    const id = routine.state.periods[cadence].id;
    if (cadence === 'monthly') {
      const [year, month] = id.split('-').map(Number);
      return new Intl.DateTimeFormat(undefined, { month: 'long', year: 'numeric', timeZone: 'UTC' }).format(new Date(Date.UTC(year, month - 1, 1)));
    }
    const start = Date.parse(`${id}T00:00:00Z`);
    if (cadence === 'daily') return new Intl.DateTimeFormat(undefined, { day: 'numeric', month: 'short', year: 'numeric', timeZone: 'UTC' }).format(start) + ' UTC';
    const end = start + 6 * 86_400_000;
    const format = new Intl.DateTimeFormat(undefined, { day: 'numeric', month: 'short', timeZone: 'UTC' });
    return `${format.format(start)}–${format.format(end)} UTC`;
  }

  let editingId = $state<string | null>(null);
  let editDraft = $state('');
  let panel = $state<HTMLElement | null>(null);

  function focusIn(selector: string): void {
    panel?.querySelector<HTMLElement>(selector)?.focus();
  }

  function startEdit(task: RoutineTask): void {
    editingId = task.id;
    editDraft = task.title;
  }

  function closeEditor(): void {
    editingId = null;
    editDraft = '';
  }

  async function cancelEdit(): Promise<void> {
    const id = editingId;
    closeEditor();
    if (!id) return;
    await tick();
    focusIn(`[data-edit-goal="${id}"]`);
  }

  async function saveEdit(): Promise<void> {
    if (!editingId) return;
    const id = editingId;
    await routine.renameMonthlyGoal(id, editDraft);
    closeEditor();
    await tick();
    focusIn(`[data-edit-goal="${id}"]`);
  }

  async function removeGoal(id: string): Promise<void> {
    const nextId = routine.state.monthlyGoals[routine.state.monthlyGoals.findIndex(goal => goal.id === id) + 1]?.id ?? null;
    await routine.removeMonthlyGoal(id);
    await tick();
    if (nextId) focusIn(`[data-remove-goal="${nextId}"]`);
    else focusIn('#monthly-routine-goal');
  }

  // Reordering keeps the keyboard on the goal being moved; when a move leaves
  // the pressed direction disabled at an end, the opposite control takes focus
  // so the next press still acts on the same goal.
  async function moveGoal(id: string, direction: -1 | 1): Promise<void> {
    await routine.moveMonthlyGoal(id, direction);
    await tick();
    const pressed = panel?.querySelector<HTMLButtonElement>(`[data-move-${direction === -1 ? 'up' : 'down'}="${id}"]`);
    if (pressed && !pressed.disabled) pressed.focus();
    else focusIn(`[data-move-${direction === -1 ? 'down' : 'up'}="${id}"]`);
  }

  function selectCadence(cadence: RoutineCadence): void {
    closeEditor();
    routine.select(cadence);
  }

  // Dragging is the pointer path; the Up/Down buttons stay the keyboard one,
  // so the handle is hidden from assistive tech rather than made focusable.
  let draggingId = $state<string | null>(null);
  let dropBefore = $state<string | null | undefined>(undefined);

  function dropTargetFor(event: DragEvent, id: string): string | null {
    const bounds = (event.currentTarget as HTMLElement).getBoundingClientRect();
    if (event.clientY - bounds.top <= bounds.height / 2) return id;
    const index = routine.state.monthlyGoals.findIndex(goal => goal.id === id);
    return routine.state.monthlyGoals[index + 1]?.id ?? null;
  }

  function startDrag(event: DragEvent, id: string): void {
    draggingId = id;
    dropBefore = undefined;
    event.dataTransfer?.setData('text/plain', id);
    const row = (event.currentTarget as HTMLElement).closest('li');
    if (row) event.dataTransfer?.setDragImage(row, 12, 12);
  }

  function endDrag(): void {
    draggingId = null;
    dropBefore = undefined;
  }

  function dragOverRow(event: DragEvent, id: string): void {
    if (!draggingId) return;
    event.preventDefault();
    if (event.dataTransfer) event.dataTransfer.dropEffect = 'move';
    dropBefore = dropTargetFor(event, id);
  }

  async function dropOnRow(event: DragEvent, id: string): Promise<void> {
    if (!draggingId) return;
    event.preventDefault();
    const dragged = draggingId;
    const target = dropTargetFor(event, id);
    endDrag();
    await routine.reorderMonthlyGoal(dragged, target);
  }

  onMount(() => routine.start());
  $effect(() => {
    const current = now;
    untrack(() => routine.refresh(current));
  });
</script>

<section class="view-header">
  <h2>Profit routines</h2>
  <p class="lede">Mark tasks as you complete them. This checklist is manual and local to this installation; TennoWorth does not detect completion from the game.</p>
</section>

<section class="wrap tw routine-checklist" aria-labelledby="routine-checklist-title" bind:this={panel}>
  <div class="rail checklist-rail">
    <h3 id="routine-checklist-title">Routine checklist</h3>
    <span class="exp" aria-live="polite">{progress}</span>
  </div>
  <div class="checklist-body">
    <div class="ui-toolbar cadence" role="group" aria-label="Checklist period">
      {#each Object.entries(cadenceLabels) as [id, label]}
        <button class="btn" class:ghost={routine.cadence !== id} class:primary={routine.cadence === id} type="button" aria-pressed={routine.cadence === id} onclick={() => selectCadence(id as RoutineCadence)}>{label}</button>
      {/each}
    </div>
    <div class="period-copy">
      <strong>{periodLabel(routine.cadence)}</strong>
      <span>{routine.cadence === 'daily' ? 'A personal day ending at 00:00 UTC; individual activities can refresh at other times.' : routine.cadence === 'weekly' ? 'Monday through Sunday. The planner starts a fresh list Monday at 00:00 UTC.' : 'Personal planning by calendar month; this is not a Warframe reset schedule.'}</span>
    </div>

    {#if routine.cadence === 'monthly'}
      <div class="ui-field monthly-goal">
        <label for="monthly-routine-goal">Add a monthly goal</label>
        <div class="goal-entry">
          <input id="monthly-routine-goal" class="ui-input" maxlength={MAX_GOAL_LENGTH} bind:value={routine.monthlyGoalDraft} placeholder="e.g. Prepare and list three ranked mods" disabled={routine.goalLimitReached} />
          <button class="btn ghost" type="button" disabled={!routine.monthlyGoalDraft.trim() || routine.goalLimitReached || routine.saving} onclick={() => void routine.addMonthlyGoal(routine.monthlyGoalDraft)}>Add goal</button>
        </div>
        <span>{routine.goalLimitReached ? `Up to ${MAX_GOALS} goals. Remove one to add another.` : 'Each goal gets its own checkbox. Goals carry forward; their ticks reset each month.'}</span>
      </div>
    {/if}

    {#if routine.tasks.length}
      <ul class="checklist-items" class:drop-end={dropBefore === null}>
        {#each routine.tasks as task (task.id)}
          <li class:done={completed.has(task.id)} class:dragging={draggingId === task.id} class:drop-before={dropBefore === task.id} ondragover={event => dragOverRow(event, task.id)} ondrop={event => void dropOnRow(event, task.id)}>
            {#if editingId === task.id}
              <div class="goal-edit">
                <input class="ui-input" aria-label="Goal text" maxlength={MAX_GOAL_LENGTH} bind:value={editDraft} />
                <button class="btn ghost" type="button" disabled={routine.saving || !editDraft.trim()} onclick={() => void saveEdit()}>Save goal</button>
                <button class="btn ghost" type="button" onclick={() => void cancelEdit()}>Cancel</button>
              </div>
            {:else}
              <div class="goal-row">
                <div class="goal-main">
                  {#if routine.cadence === 'monthly'}
                    <span class="drag-handle" data-drag-handle={task.id} draggable="true" aria-hidden="true" ondragstart={event => startDrag(event, task.id)} ondragend={endDrag}>⠿</span>
                  {/if}
                  <label>
                    <input type="checkbox" checked={completed.has(task.id)} onchange={event => void routine.toggle(task.id, event.currentTarget.checked)} />
                    <span class="task-copy"><strong>{task.title}</strong><span>{task.detail}</span></span>
                  </label>
                </div>
                {#if routine.cadence === 'monthly'}
                  <div class="row-actions">
                    <button class="btn ghost xs" type="button" data-move-up={task.id} aria-label={`Move ${task.title} up`} disabled={routine.state.monthlyGoals[0]?.id === task.id} onclick={() => void moveGoal(task.id, -1)}>Up</button>
                    <button class="btn ghost xs" type="button" data-move-down={task.id} aria-label={`Move ${task.title} down`} disabled={routine.state.monthlyGoals.at(-1)?.id === task.id} onclick={() => void moveGoal(task.id, 1)}>Down</button>
                    <button class="btn ghost xs" type="button" data-edit-goal={task.id} aria-label={`Edit ${task.title}`} onclick={() => startEdit(task)}>Edit</button>
                    <button class="btn ghost xs" type="button" data-remove-goal={task.id} aria-label={`Remove ${task.title}`} disabled={routine.saving} onclick={() => void removeGoal(task.id)}>Remove</button>
                  </div>
                {/if}
              </div>
            {/if}
          </li>
        {/each}
      </ul>
    {:else}
      <p class="empty-goal">Add a monthly goal to build this month’s checklist.</p>
    {/if}

    {#if routine.saveError}
      <div class="ui-notice save-error" data-tone="bad" role="alert">
        <span>{routine.saveError}</span>
        <button class="btn ghost" type="button" disabled={routine.saving} onclick={() => void routine.retry()}>{routine.saving ? 'Saving…' : 'Retry saving'}</button>
      </div>
    {:else if routine.saving}
      <p class="save-status" role="status">Saving progress…</p>
    {/if}
  </div>
</section>

<section class="card ui-panel routine routine-timing" aria-label="Routine timing">
  <div class="routine-clocks">
    <div class="clock">
      <span class="clock-label">New daily checklist</span>
      <strong class="clock-val">{humanWindow(nextRoutinePeriod('daily', now) - now)}</strong>
      <span class="clock-sub">00:00 UTC</span>
    </div>
    <div class="clock">
      <span class="clock-label">New weekly checklist</span>
      <strong class="clock-val">{humanWindow(nextRoutinePeriod('weekly', now) - now)}</strong>
      <span class="clock-sub">Mon 00:00 UTC</span>
    </div>
    <div class="clock">
      <span class="clock-label">{baroWindow?.label ?? 'Next Baro visit'}</span>
      <strong class="clock-val">{baroWindow ? humanWindow(baroWindow.duration) : '-'}</strong>
      <span class="clock-sub">{baro?.location ?? 'schedule unknown'}</span>
    </div>
  </div>
</section>

<section class="baro-calendar">
  <TraderCalendar {market} {owned} {now} />
</section>

<details class="wrap tw routine-advice">
  <summary>Endo and selling notes</summary>
  <div class="advice-body">
    <p>Baro follows the visit schedule above rather than the weekly checklist. Review his stock when he arrives, and plan purchases around your own inventory.</p>
    <p>For Endo, compare the time and difficulty of Arbitrations, Rathuum, excavation, Sorties, Archon Hunts, and Maroo’s weekly treasure hunt. Account-bound rewards can save Platinum even when they cannot be sold.</p>
  </div>
</details>

<style>
  .routine-checklist, .routine-advice { padding: 0; }
  .checklist-rail { min-height: var(--rail); }
  .checklist-body { display: flex; flex-direction: column; gap: var(--s4); padding: var(--s4) var(--inset); }
  .cadence { gap: var(--s2); }
  .cadence .btn { min-width: 6rem; }
  .period-copy { display: flex; flex-direction: column; gap: var(--s1); }
  .period-copy strong { font-family: var(--font-ui); font-size: var(--text-section); }
  .period-copy span, .monthly-goal > span, .save-status, .empty-goal, .advice-body { color: var(--muted); font-size: var(--text-control); line-height: var(--leading-body); }
  .goal-entry { display: flex; gap: var(--s2); align-items: stretch; }
  .goal-entry input { flex: 1 1 20rem; min-width: 0; }
  .goal-row { display: flex; align-items: flex-start; gap: var(--s3); }
  .goal-main { display: flex; align-items: flex-start; gap: var(--s3); flex: 1 1 auto; min-width: 0; }
  .goal-main > label { flex: 1 1 auto; min-width: 0; }
  .drag-handle { display: flex; align-items: center; align-self: stretch; padding-block: var(--s3); color: var(--muted); cursor: grab; user-select: none; }
  .drag-handle:active { cursor: grabbing; }
  .checklist-items li.dragging { background: var(--panel-2); }
  .checklist-items li.drop-before { box-shadow: inset 0 2px 0 var(--accent); }
  .checklist-items.drop-end { border-bottom: 2px solid var(--accent); }
  .row-actions { display: flex; flex-wrap: wrap; flex-shrink: 0; gap: var(--s2); padding-block: var(--s3); }
  .goal-edit { display: flex; align-items: center; gap: var(--s2); padding-block: var(--s3); }
  .goal-edit input { flex: 1 1 20rem; min-width: 0; }
  .checklist-items { list-style: none; margin: 0; padding: 0; border-top: 1px var(--rule) var(--border); }
  .checklist-items li { border-bottom: 1px var(--rule) var(--border); }
  .checklist-items label { display: flex; align-items: flex-start; gap: var(--s3); min-height: var(--row); padding: var(--s3) 0; cursor: pointer; }
  .checklist-items input[type="checkbox"] { flex: 0 0 auto; width: 1.25rem; height: 1.25rem; margin: var(--s1) 0 0; }
  .task-copy { display: flex; flex-direction: column; gap: var(--s1); min-width: 0; overflow-wrap: anywhere; line-height: var(--leading-body); }
  .task-copy strong { font-weight: 600; }
  .task-copy span { color: var(--muted); font-size: var(--text-control); }
  li.done .task-copy strong { text-decoration: line-through; }
  li.done .task-copy::after { content: 'Completed'; color: var(--good); font: var(--text-caption)/var(--leading-control) var(--font-ui); text-transform: uppercase; letter-spacing: 0.04em; }
  .save-error { display: flex; align-items: center; justify-content: space-between; gap: var(--s3); }
  .save-status, .empty-goal { margin: 0; }
  .routine-clocks { display: grid; grid-template-columns: repeat(auto-fit, minmax(min(15rem, 100%), 1fr)); }
  .clock { display: flex; flex-direction: column; gap: var(--s1); border-right: 1px var(--rule) var(--border); padding: var(--s4) var(--inset); background: var(--panel); }
  .clock:last-child { border-right: 0; }
  .clock-label { color: var(--muted); font-size: var(--text-caption); letter-spacing: 0.04em; text-transform: uppercase; }
  .clock-val { color: var(--fg); font-size: var(--text-heading); font-weight: 600; }
  .clock-sub { color: var(--muted); font-size: var(--text-caption); }
  .routine-advice > summary { cursor: pointer; min-height: var(--bar); padding: var(--s3) var(--inset); background: var(--ink-bar); color: var(--on-ink); font-family: var(--font-ui); font-size: var(--text-caption); letter-spacing: 0.03em; text-transform: uppercase; }
  .routine-advice > summary:focus-visible { outline-color: var(--on-ink); outline-offset: -3px; }
  .advice-body { display: flex; flex-direction: column; gap: var(--s3); padding: var(--s4) var(--inset); }
  .advice-body p { margin: 0; }
  @media (max-width: 47.5rem) {
    .goal-entry, .save-error { align-items: stretch; flex-direction: column; }
    /* The 20rem flex basis sizes a row; stacked it would become a 320px-tall field. */
    .goal-entry input { flex: 0 0 auto; }
    .goal-entry .btn, .save-error .btn { align-self: flex-start; }
    .goal-row, .goal-edit { flex-wrap: wrap; }
    .row-actions { padding-block: 0 var(--s3); }
    .routine-clocks { grid-template-columns: 1fr; }
    .clock { border-right: 0; border-bottom: 1px var(--rule) var(--border); }
    .clock:last-child { border-bottom: 0; }
  }
</style>
