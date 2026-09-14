import { describe, expect, it, vi } from 'vitest';
import type { StateStore, SettingKey } from '../../contracts/state-store';
import { MAX_GOALS, MAX_GOAL_LENGTH, parseRoutineState, routinePeriodId, RoutineController } from './controller.svelte';

function memoryStore(initial: string | null = null, write = async () => {}): StateStore & { values: Map<string, string>; writes: string[] } {
  const values = new Map<string, string>();
  if (initial !== null) values.set('routine-checklist', initial);
  const writes: string[] = [];
  return {
    mode: 'local', values, writes,
    hydrate: async () => {},
    getSetting: key => values.get(key) ?? null,
    setSetting: async (key: SettingKey, value: string) => { writes.push(value); await write(); values.set(key, value); },
    loadSnapshot: async () => null,
    saveSnapshot: async () => {},
    clearSnapshot: async () => {},
  };
}

const NOW = Date.parse('2026-09-14T12:00:00Z');

function numberedGoals(count: number) {
  return Array.from({ length: count }, (_, index) => ({ id: `goal-${index + 1}`, text: `goal ${index + 1}` }));
}

describe('RoutineController', () => {
  it('uses UTC calendar periods across day, Monday-week, month, and year boundaries', () => {
    const sunday = Date.parse('2026-12-27T23:59:59Z');
    const monday = Date.parse('2026-12-28T00:00:00Z');
    const newYear = Date.parse('2027-01-01T00:00:00Z');
    expect(routinePeriodId('daily', sunday)).toBe('2026-12-27');
    expect(routinePeriodId('weekly', sunday)).toBe('2026-12-21');
    expect(routinePeriodId('weekly', monday)).toBe('2026-12-28');
    expect(routinePeriodId('monthly', newYear)).toBe('2027-01');
  });

  it('repairs corrupt input without crashing', () => {
    const parsed = parseRoutineState('{bad json', NOW);
    expect(parsed.repaired).toBe(true);
    expect(parsed.state.periods.daily).toEqual({ id: '2026-09-14', completed: [] });
    expect(parsed.state.monthlyGoals).toEqual([]);
  });

  it('rejects the retired single-goal shape', () => {
    const parsed = parseRoutineState(JSON.stringify({
      version: 1,
      periods: { daily: { id: '2026-09-14', completed: [] }, weekly: { id: '2026-09-14', completed: [] }, monthly: { id: '2026-09', completed: ['personal-goal'] } },
      monthlyGoal: 'Testium',
    }), NOW);
    expect(parsed.repaired).toBe(true);
    expect(parsed.state.monthlyGoals).toEqual([]);
  });

  it('drops malformed goals, duplicate ids, over-length text, and unknown completions', () => {
    const parsed = parseRoutineState(JSON.stringify({
      version: 2,
      periods: {
        daily: { id: '2026-09-14', completed: ['sortie', 'sortie', 'removed-task', 42] },
        weekly: { id: '2026-09-14', completed: 'archon-hunt' },
        monthly: { id: '2026-09', completed: ['goal-2', 'ghost', 'goal-2', 'goal-4'] },
      },
      monthlyGoals: [
        { id: 'goal-2', text: '  List ranked mods  ' },
        { id: 'goal-2', text: 'duplicate id' },
        { id: 'goal-3', text: 'x'.repeat(200) },
        { id: 'goal-4' },
        'nope',
        { id: 'goal-5', text: '   ' },
        { id: 'goal-1', text: 'kept first' },
      ],
      nextGoalId: 2,
    }), NOW);
    expect(parsed.repaired).toBe(true);
    expect(parsed.state.monthlyGoals).toEqual([
      { id: 'goal-2', text: 'List ranked mods' },
      { id: 'goal-3', text: 'x'.repeat(MAX_GOAL_LENGTH) },
      { id: 'goal-1', text: 'kept first' },
    ]);
    expect(parsed.state.periods.daily.completed).toEqual(['sortie']);
    expect(parsed.state.periods.weekly.completed).toEqual([]);
    expect(parsed.state.periods.monthly.completed).toEqual(['goal-2']);
    expect(parsed.state.nextGoalId).toBe(6);
  });

  it('caps restored goals and hands out ids that cannot collide with them', async () => {
    const parsed = parseRoutineState(JSON.stringify({
      version: 2,
      periods: {},
      monthlyGoals: numberedGoals(MAX_GOALS + 4),
      nextGoalId: 1,
    }), NOW);
    expect(parsed.state.monthlyGoals).toHaveLength(MAX_GOALS);
    expect(parsed.state.nextGoalId).toBe(MAX_GOALS + 5);

    const routine = new RoutineController(memoryStore(JSON.stringify(parsed.state)), NOW);
    routine.select('monthly');
    await routine.addMonthlyGoal('refused at the restored cap');
    expect(routine.state.monthlyGoals).toHaveLength(MAX_GOALS);
  });

  it('adds goals with independent ids and ticks', async () => {
    const store = memoryStore();
    const routine = new RoutineController(store, NOW);
    routine.select('monthly');
    await routine.addMonthlyGoal('List three ranked mods');
    await routine.addMonthlyGoal('Prepare Prime sets');

    const [first, second] = routine.state.monthlyGoals;
    expect(routine.tasks.map(task => task.title)).toEqual(['List three ranked mods', 'Prepare Prime sets']);
    expect(routine.monthlyGoalDraft).toBe('');

    await routine.toggle(first.id, true);
    expect(routine.completed).toEqual(new Set([first.id]));
    expect(JSON.parse(store.values.get('routine-checklist')!).monthlyGoals).toHaveLength(2);
    expect(second.id).not.toBe(first.id);
  });

  it('refuses an empty add, truncates long text, and stops at the goal cap', async () => {
    const routine = new RoutineController(memoryStore(), NOW);
    routine.select('monthly');
    await routine.addMonthlyGoal('   ');
    expect(routine.state.monthlyGoals).toEqual([]);

    await routine.addMonthlyGoal('y'.repeat(200));
    expect(routine.state.monthlyGoals[0].text).toHaveLength(MAX_GOAL_LENGTH);

    for (let index = routine.state.monthlyGoals.length; index < MAX_GOALS; index++) await routine.addMonthlyGoal(`goal ${index}`);
    expect(routine.state.monthlyGoals).toHaveLength(MAX_GOALS);
    expect(routine.goalLimitReached).toBe(true);

    await routine.addMonthlyGoal('one too many');
    expect(routine.state.monthlyGoals).toHaveLength(MAX_GOALS);
    expect(routine.state.monthlyGoals.at(-1)!.text).not.toBe('one too many');
  });

  it('keeps a renamed goal tick when the text is unchanged and clears only it when it changes', async () => {
    const routine = new RoutineController(memoryStore(), NOW);
    routine.select('monthly');
    await routine.addMonthlyGoal('List ranked mods');
    await routine.addMonthlyGoal('Prepare Prime sets');
    const [first, second] = routine.state.monthlyGoals;
    await routine.toggle(first.id, true);
    await routine.toggle(second.id, true);

    await routine.renameMonthlyGoal(first.id, `  ${first.text}  `);
    expect(routine.completed).toEqual(new Set([first.id, second.id]));

    await routine.renameMonthlyGoal(first.id, 'List four ranked mods');
    expect(routine.tasks[0].title).toBe('List four ranked mods');
    expect(routine.completed).toEqual(new Set([second.id]));
  });

  it('removes only the chosen goal and its tick, and empties cleanly', async () => {
    const routine = new RoutineController(memoryStore(), NOW);
    routine.select('monthly');
    await routine.addMonthlyGoal('List ranked mods');
    await routine.addMonthlyGoal('Prepare Prime sets');
    const [first, second] = routine.state.monthlyGoals;
    await routine.toggle(first.id, true);
    await routine.toggle(second.id, true);

    await routine.removeMonthlyGoal(first.id);
    expect(routine.state.monthlyGoals).toEqual([second]);
    expect(routine.completed).toEqual(new Set([second.id]));

    await routine.removeMonthlyGoal(second.id);
    expect(routine.state.monthlyGoals).toEqual([]);
    expect(routine.tasks).toEqual([]);
    expect(routine.goalLimitReached).toBe(false);
  });

  it('reorders goals without touching their text or ticks', async () => {
    const store = memoryStore();
    const routine = new RoutineController(store, NOW);
    routine.select('monthly');
    for (const text of ['first', 'second', 'third']) await routine.addMonthlyGoal(text);
    const [first, second, third] = routine.state.monthlyGoals;
    await routine.toggle(second.id, true);

    await routine.moveMonthlyGoal(third.id, -1);
    expect(routine.state.monthlyGoals.map(goal => goal.text)).toEqual(['first', 'third', 'second']);
    expect(routine.completed).toEqual(new Set([second.id]));
    expect(JSON.parse(store.values.get('routine-checklist')!).monthlyGoals.map((goal: { text: string }) => goal.text))
      .toEqual(['first', 'third', 'second']);

    await routine.moveMonthlyGoal(third.id, 1);
    expect(routine.state.monthlyGoals.map(goal => goal.text)).toEqual(['first', 'second', 'third']);
    expect(routine.state.monthlyGoals.map(goal => goal.id)).toEqual([first.id, second.id, third.id]);
    expect(routine.completed).toEqual(new Set([second.id]));
  });

  it('refuses a move past either end and ignores an unknown goal', async () => {
    const store = memoryStore();
    const routine = new RoutineController(store, NOW);
    routine.select('monthly');
    await routine.addMonthlyGoal('first');
    await routine.addMonthlyGoal('second');
    const [first, second] = routine.state.monthlyGoals;
    const writes = store.writes.length;

    await routine.moveMonthlyGoal(first.id, -1);
    await routine.moveMonthlyGoal(second.id, 1);
    await routine.moveMonthlyGoal('goal-999', 1);
    expect(routine.state.monthlyGoals.map(goal => goal.text)).toEqual(['first', 'second']);
    expect(store.writes).toHaveLength(writes);
  });

  it('rolls the week and the month without losing the goals or their order', async () => {
    const routine = new RoutineController(memoryStore(), Date.parse('2026-09-20T23:59:00Z'));
    await routine.toggle('login-tribute', true);
    routine.select('weekly');
    await routine.toggle('archon-hunt', true);
    routine.select('monthly');
    await routine.addMonthlyGoal('Prepare Prime sets');
    await routine.addMonthlyGoal('List ranked mods');
    await routine.toggle(routine.state.monthlyGoals[1].id, true);

    routine.refresh(Date.parse('2026-09-21T00:01:00Z'));
    await vi.waitFor(() => expect(routine.saving).toBe(false));
    expect(routine.state.periods.daily.completed).toEqual([]);
    expect(routine.state.periods.weekly.completed).toEqual([]);
    expect(routine.state.monthlyGoals.map(goal => goal.text)).toEqual(['Prepare Prime sets', 'List ranked mods']);
    expect(routine.completed.size).toBe(1);

    routine.refresh(Date.parse('2026-10-01T00:01:00Z'));
    await vi.waitFor(() => expect(routine.saving).toBe(false));
    expect(routine.state.periods.monthly.completed).toEqual([]);
    expect(routine.state.monthlyGoals.map(goal => goal.text)).toEqual(['Prepare Prime sets', 'List ranked mods']);
  });

  it('reports a failed write and retries the latest rapid change serially', async () => {
    let releaseFirst!: () => void;
    let attempts = 0;
    const store = memoryStore(null, async () => {
      attempts++;
      if (attempts === 1) await new Promise<void>(resolve => { releaseFirst = resolve; });
      if (attempts <= 2) throw new Error('disk unavailable');
    });
    const routine = new RoutineController(store, NOW);
    const first = routine.toggle('login-tribute', true);
    const second = routine.toggle('sortie', true);
    releaseFirst();
    await Promise.all([first, second]);
    expect(routine.saveError).toMatch(/only saved in this open view/);
    expect(routine.completed).toEqual(new Set(['login-tribute', 'sortie']));

    await routine.retry();
    expect(routine.saveError).toBe('');
    const persisted = JSON.parse(store.values.get('routine-checklist')!);
    expect(persisted.periods.daily.completed).toEqual(['login-tribute', 'sortie']);
    expect(store.writes).toHaveLength(3);
  });

  it('detects a browser adapter write that resolves without retaining data', async () => {
    const store = memoryStore();
    store.setSetting = async (_key, value) => { store.writes.push(value); };
    const routine = new RoutineController(store, NOW);
    await routine.toggle('sortie', true);
    expect(routine.saveError).toMatch(/Retry before closing/);
  });
});
