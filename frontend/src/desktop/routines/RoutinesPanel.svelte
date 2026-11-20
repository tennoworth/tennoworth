<script lang="ts">
  import { roving } from '../../ui/keyboard-actions';
  import { onMount, tick, untrack } from 'svelte';
  import type { Market, OwnedRecord } from '../../contracts/data';
  import { baroLocation, humanWindow } from '../../ui/format';
  import { formatLocale, t } from '../../i18n/i18n.svelte';
  import TraderCalendar from '../../features/calendar/TraderCalendar.svelte';
  import { MAX_GOALS, MAX_GOAL_LENGTH, MAX_TASKS, nextRoutinePeriod, type RoutineController, type RoutineCadence, type RoutineTask } from './controller.svelte';

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
      return { label: t('routine.baroLeaves'), duration: expiry - now };
    }
    if (Number.isFinite(arrival) && now < arrival) return { label: t('routine.baroArrives'), duration: arrival - now };
    return null;
  });
  let completed = $derived(routine.completed);
  let progress = $derived(t('routine.progress', { done: completed.size, total: routine.tasks.length }));

  const CADENCES: RoutineCadence[] = ['daily', 'weekly', 'monthly'];

  function periodLabel(cadence: RoutineCadence): string {
    const id = routine.state.periods[cadence].id;
    if (cadence === 'monthly') {
      const [year, month] = id.split('-').map(Number);
      return new Intl.DateTimeFormat(formatLocale(), { month: 'long', year: 'numeric', timeZone: 'UTC' }).format(new Date(Date.UTC(year, month - 1, 1)));
    }
    const start = Date.parse(`${id}T00:00:00Z`);
    if (cadence === 'daily') return new Intl.DateTimeFormat(formatLocale(), { day: 'numeric', month: 'short', year: 'numeric', timeZone: 'UTC' }).format(start) + ' UTC';
    const end = start + 6 * 86_400_000;
    const format = new Intl.DateTimeFormat(formatLocale(), { day: 'numeric', month: 'short', timeZone: 'UTC' });
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

  // Renaming a built-in daily or weekly item gives it a new id in the same
  // place, so focus returns by position rather than by id.
  async function saveEdit(): Promise<void> {
    if (!editingId) return;
    const id = editingId;
    const index = routine.tasks.findIndex(task => task.id === id);
    await (routine.cadence === 'monthly' ? routine.renameMonthlyGoal(id, editDraft) : routine.renameTask(id, editDraft));
    closeEditor();
    await tick();
    focusIn(`[data-edit-goal="${routine.tasks[index]?.id ?? id}"]`);
  }

  async function removeGoal(id: string): Promise<void> {
    const nextId = routine.state.monthlyGoals[routine.state.monthlyGoals.findIndex(goal => goal.id === id) + 1]?.id ?? null;
    await routine.removeMonthlyGoal(id);
    await tick();
    if (nextId) focusIn(`[data-remove-goal="${nextId}"]`);
    else focusIn('#monthly-routine-goal');
  }

  let taskDraft = $state('');

  // Removing a row hands focus to the next row's Remove, or to the add area
  // when it was the last one, so the keyboard never drops to the page.
  async function removeTask(id: string): Promise<void> {
    const ids = routine.tasks.map(task => task.id);
    const nextId = ids[ids.indexOf(id) + 1] ?? null;
    await routine.removeTask(id);
    await tick();
    if (nextId) focusIn(`[data-remove-task="${nextId}"]`);
    else focusIn('[data-add-builtin], #routine-task');
  }

  // An added activity's button leaves the row; the one that slides into its
  // place takes focus, or the custom field once the row is empty or full.
  async function addBuiltin(id: string, index: number): Promise<void> {
    await routine.addBuiltinTask(id);
    await tick();
    const left = panel?.querySelectorAll<HTMLElement>('[data-add-builtin]') ?? [];
    const next = left[Math.min(index, left.length - 1)];
    if (next && !routine.taskLimitReached) next.focus();
    else focusIn('#routine-task');
  }

  async function addTask(): Promise<void> {
    await routine.addCustomTask(taskDraft);
    taskDraft = '';
  }

  // Reordering keeps the keyboard on the goal being moved; when a move leaves
  // the pressed direction disabled at an end, the opposite control takes focus
  // so the next press still acts on the same goal.
  async function moveGoal(id: string, direction: -1 | 1): Promise<void> {
    await (routine.cadence === 'monthly' ? routine.moveMonthlyGoal(id, direction) : routine.moveTask(id, direction));
    await tick();
    const pressed = panel?.querySelector<HTMLButtonElement>(`[data-move-${direction === -1 ? 'up' : 'down'}="${id}"]`);
    if (pressed && !pressed.disabled) pressed.focus();
    else focusIn(`[data-move-${direction === -1 ? 'down' : 'up'}="${id}"]`);
  }

  function selectCadence(cadence: RoutineCadence): void {
    closeEditor();
    taskDraft = '';
    routine.select(cadence);
  }

  // Dragging is the pointer path; the Up/Down buttons stay the keyboard one,
  // so the handle is hidden from assistive tech rather than made focusable.
  let draggingId = $state<string | null>(null);
  let dropBefore = $state<string | null | undefined>(undefined);

  function dropTargetFor(event: DragEvent, id: string): string | null {
    const bounds = (event.currentTarget as HTMLElement).getBoundingClientRect();
    if (event.clientY - bounds.top <= bounds.height / 2) return id;
    const ids = routine.tasks.map(task => task.id);
    return ids[ids.indexOf(id) + 1] ?? null;
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
    await (routine.cadence === 'monthly' ? routine.reorderMonthlyGoal(dragged, target) : routine.reorderTask(dragged, target));
  }

  onMount(() => routine.start());
  $effect(() => {
    const current = now;
    untrack(() => routine.refresh(current));
  });
</script>

<section class="view-header">
  <h2>{t('routine.title')}</h2>
  <p class="lede">{t('routine.lede')}</p>
</section>

<section class="wrap tw routine-checklist" aria-labelledby="routine-checklist-title" bind:this={panel}>
  <div class="rail checklist-rail">
    <h3 id="routine-checklist-title">{t('routine.checklist')}</h3>
    <span class="exp" aria-live="polite">{progress}</span>
  </div>
  <div class="checklist-body">
    <div class="ui-segmented cadence" role="group" aria-label={t('routine.period')} use:roving>
      {#each CADENCES as id (id)}
        <button type="button" aria-pressed={routine.cadence === id} onclick={() => selectCadence(id)}>{t(`routine.cadence.${id}`)}</button>
      {/each}
    </div>
    <div class="period-copy">
      <strong>{periodLabel(routine.cadence)}</strong>
      <span>{t(`routine.about.${routine.cadence}`)}</span>
    </div>

    {#if routine.cadence === 'monthly'}
      <div class="ui-field monthly-goal">
        <label for="monthly-routine-goal">{t('routine.addGoalLabel')}</label>
        <div class="goal-entry">
          <input id="monthly-routine-goal" class="ui-input" maxlength={MAX_GOAL_LENGTH} bind:value={routine.monthlyGoalDraft} placeholder={t('routine.goalExample')} disabled={routine.goalLimitReached} />
          <button class="btn ghost" type="button" disabled={!routine.monthlyGoalDraft.trim() || routine.goalLimitReached || routine.saving} onclick={() => void routine.addMonthlyGoal(routine.monthlyGoalDraft)}>{t('routine.addGoal')}</button>
        </div>
        <span>{routine.goalLimitReached ? t('routine.goalLimit', { max: MAX_GOALS }) : t('routine.goalHelp')}</span>
      </div>
    {/if}

    {#if routine.tasks.length}
      <ul class="checklist-items" class:drop-end={dropBefore === null}>
        {#each routine.tasks as task (task.id)}
          <li class:done={completed.has(task.id)} class:dragging={draggingId === task.id} class:drop-before={dropBefore === task.id} ondragover={event => dragOverRow(event, task.id)} ondrop={event => void dropOnRow(event, task.id)}>
            {#if editingId === task.id}
              <div class="goal-edit">
                <input class="ui-input" aria-label={t(routine.cadence === 'monthly' ? 'routine.goalText' : 'routine.itemText')} maxlength={MAX_GOAL_LENGTH} bind:value={editDraft} />
                <button class="btn ghost" type="button" disabled={routine.saving || !editDraft.trim()} onclick={() => void saveEdit()}>{t(routine.cadence === 'monthly' ? 'routine.saveGoal' : 'routine.saveItem')}</button>
                <button class="btn ghost" type="button" onclick={() => void cancelEdit()}>{t('routine.cancel')}</button>
              </div>
            {:else}
              <div class="goal-row">
                <div class="goal-main">
                  <span class="drag-handle" data-drag-handle={task.id} draggable="true" aria-hidden="true" ondragstart={event => startDrag(event, task.id)} ondragend={endDrag}>⠿</span>
                  <label>
                    <input type="checkbox" checked={completed.has(task.id)} onchange={event => void routine.toggle(task.id, event.currentTarget.checked)} />
                    <span class="task-copy"><strong>{task.title}</strong><span>{task.detail}</span></span>
                  </label>
                </div>
                <div class="row-actions">
                  <button class="btn ghost xs" type="button" data-move-up={task.id} aria-label={t('routine.moveUp', { goal: task.title })} disabled={routine.tasks[0]?.id === task.id} onclick={() => void moveGoal(task.id, -1)}>{t('routine.up')}</button>
                  <button class="btn ghost xs" type="button" data-move-down={task.id} aria-label={t('routine.moveDown', { goal: task.title })} disabled={routine.tasks.at(-1)?.id === task.id} onclick={() => void moveGoal(task.id, 1)}>{t('routine.down')}</button>
                  <button class="btn ghost xs" type="button" data-edit-goal={task.id} aria-label={t('routine.edit', { goal: task.title })} onclick={() => startEdit(task)}>{t('routine.editShort')}</button>
                  {#if routine.cadence === 'monthly'}
                    <button class="btn ghost xs" type="button" data-remove-goal={task.id} aria-label={t('routine.remove', { goal: task.title })} disabled={routine.saving} onclick={() => void removeGoal(task.id)}>{t('routine.removeShort')}</button>
                  {:else}
                    <button class="btn ghost xs" type="button" data-remove-task={task.id} aria-label={t('routine.remove', { goal: task.title })} disabled={routine.saving} onclick={() => void removeTask(task.id)}>{t('routine.removeShort')}</button>
                  {/if}
                </div>
              </div>
            {/if}
          </li>
        {/each}
      </ul>
    {:else}
      <p class="empty-goal">{t(routine.cadence === 'monthly' ? 'routine.emptyGoals' : 'routine.emptyTasks')}</p>
    {/if}

    {#if routine.cadence !== 'monthly'}
      <div class="add-tasks">
        <div class="ui-field">
          <span id="routine-from-game">{t('routine.fromGame')}</span>
          {#if routine.availableBuiltins.length}
            <div class="builtins" role="group" aria-labelledby="routine-from-game" use:roving>
              {#each routine.availableBuiltins as task, index (task.id)}
                <button class="btn ghost xs add" type="button" data-add-builtin={task.id} title={task.detail} aria-label={t('routine.addBuiltin', { task: task.title })} disabled={routine.taskLimitReached || routine.saving} onclick={() => void addBuiltin(task.id, index)}>+ {task.title}</button>
              {/each}
            </div>
          {:else}
            <span class="note">{t('routine.allListed')}</span>
          {/if}
        </div>
        <div class="ui-field">
          <label for="routine-task">{t('routine.addTaskLabel')}</label>
          <div class="goal-entry">
            <input id="routine-task" class="ui-input" maxlength={MAX_GOAL_LENGTH} bind:value={taskDraft} placeholder={t('routine.taskExample')} disabled={routine.taskLimitReached} onkeydown={event => { if (event.key === 'Enter' && taskDraft.trim()) void addTask(); }} />
            <button class="btn ghost" type="button" disabled={!taskDraft.trim() || routine.taskLimitReached || routine.saving} onclick={() => void addTask()}>{t('routine.addTask')}</button>
          </div>
          <span class="note">{routine.taskLimitReached ? t('routine.taskLimit', { max: MAX_TASKS }) : t('routine.taskHelp')}</span>
        </div>
      </div>
    {/if}

    {#if routine.saveError}
      <div class="ui-notice save-error" data-tone="bad" role="alert">
        <span>{routine.saveError}</span>
        <button class="btn ghost" type="button" disabled={routine.saving} onclick={() => void routine.retry()}>{t(routine.saving ? 'routine.saving' : 'routine.retrySave')}</button>
      </div>
    {:else if routine.saving}
      <p class="save-status" role="status">{t('routine.savingProgress')}</p>
    {/if}
  </div>
</section>

<section class="card ui-panel routine routine-timing" aria-label={t('routine.timing')}>
  <div class="routine-clocks">
    <div class="clock">
      <span class="clock-label">{t('routine.newDaily')}</span>
      <strong class="clock-val">{humanWindow(nextRoutinePeriod('daily', now) - now)}</strong>
      <span class="clock-sub">00:00 UTC</span>
    </div>
    <div class="clock">
      <span class="clock-label">{t('routine.newWeekly')}</span>
      <strong class="clock-val">{humanWindow(nextRoutinePeriod('weekly', now) - now)}</strong>
      <span class="clock-sub">{t('routine.mondayReset')}</span>
    </div>
    <div class="clock">
      <span class="clock-label">{baroWindow?.label ?? t('routine.nextBaro')}</span>
      <strong class="clock-val">{baroWindow ? humanWindow(baroWindow.duration) : '-'}</strong>
      <span class="clock-sub">{baro?.location ?? t('routine.scheduleUnknown')}</span>
    </div>
  </div>
</section>

<section class="baro-calendar">
  <TraderCalendar {market} {owned} {now} />
</section>

<details class="wrap tw routine-advice">
  <summary>{t('routine.notes')}</summary>
  <div class="advice-body">
    <p>{t('routine.notes1')}</p>
    <p>{t('routine.notes2')}</p>
  </div>
</details>

<style>
  .routine-checklist, .routine-advice { padding: 0; }
  .checklist-rail { min-height: var(--rail); }
  .checklist-body { display: flex; flex-direction: column; gap: var(--s4); padding: var(--s4) var(--inset); }
  .cadence { align-self: flex-start; }
  .cadence > button { min-width: 6rem; justify-content: center; }
  .period-copy { display: flex; flex-direction: column; gap: var(--s1); }
  .period-copy strong { font-family: var(--font-ui); font-size: var(--text-section); }
  .period-copy span, .monthly-goal > span, .save-status, .empty-goal, .advice-body { color: var(--muted); font-size: var(--text-control); line-height: var(--leading-body); }
  .goal-entry { display: flex; gap: var(--s2); align-items: stretch; }
  .add-tasks { display: flex; flex-direction: column; gap: var(--s4); }
  .add-tasks .note { color: var(--muted); font-size: var(--text-control); line-height: var(--leading-body); }
  .builtins { display: flex; flex-wrap: wrap; gap: var(--s2); }
  /* Dashed is the add action's border (design system: Boundaries). */
  .builtins .add { border-style: dashed; }
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
  .checklist-items { list-style: none; margin: 0; padding: 0; border-top: 1px var(--rule) var(--rule-ink); }
  .checklist-items li { border-bottom: 1px var(--rule) var(--rule-ink); }
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
  .clock { display: flex; flex-direction: column; gap: var(--s1); border-right: 1px var(--rule) var(--rule-ink); padding: var(--s4) var(--inset); background: var(--panel); }
  .clock:last-child { border-right: 0; }
  .clock-label { color: var(--muted); font-size: var(--text-caption); letter-spacing: 0.04em; text-transform: uppercase; }
  .clock-val { color: var(--fg); font-size: var(--text-heading); font-weight: 600; }
  .clock-sub { color: var(--muted); font-size: var(--text-caption); }
  .routine-advice > summary { cursor: pointer; min-height: var(--bar); padding: var(--s3) var(--inset); background: var(--rail-bg); color: var(--rail-fg); box-shadow: inset 0 2px 0 var(--rail-edge); font-family: var(--font-ui); font-size: var(--text-caption); letter-spacing: 0.03em; text-transform: uppercase; }
  .routine-advice > summary::before { color: var(--rail-muted); }
  .routine-advice > summary:focus-visible { outline-color: var(--rail-fg); outline-offset: -3px; }
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
    .clock { border-right: 0; border-bottom: 1px var(--rule) var(--rule-ink); }
    .clock:last-child { border-bottom: 0; }
  }
</style>
