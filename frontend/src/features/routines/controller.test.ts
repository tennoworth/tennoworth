import { describe, expect, it, vi } from 'vitest';
import type { StateStore, SettingKey } from '../../contracts/state-store';
import { parseRoutineState, routinePeriodId, RoutineController } from './controller.svelte';

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
    const now = Date.parse('2026-09-14T12:00:00Z');
    const parsed = parseRoutineState('{bad json', now);
    expect(parsed.repaired).toBe(true);
    expect(parsed.state.periods.daily).toEqual({ id: '2026-09-14', completed: [] });
    expect(parsed.state.monthlyGoal).toBe('');
  });

  it('drops malformed buckets, duplicate completions, and unknown task IDs', () => {
    const now = Date.parse('2026-09-14T12:00:00Z');
    const parsed = parseRoutineState(JSON.stringify({
      version: 1,
      periods: {
        daily: { id: '2026-09-14', completed: ['sortie', 'sortie', 'removed-task', 42] },
        weekly: { id: '2026-09-14', completed: 'archon-hunt' },
        monthly: { id: '2026-09', completed: ['personal-goal'] },
      },
      monthlyGoal: '',
    }), now);
    expect(parsed.repaired).toBe(true);
    expect(parsed.state.periods.daily.completed).toEqual(['sortie']);
    expect(parsed.state.periods.weekly.completed).toEqual([]);
    expect(parsed.state.periods.monthly.completed).toEqual([]);
  });

  it('starts a new Monday checklist without clearing the current monthly goal', async () => {
    const store = memoryStore();
    const routine = new RoutineController(store, Date.parse('2026-09-20T23:59:00Z'));
    routine.select('weekly');
    await routine.toggle('archon-hunt', true);
    routine.select('monthly');
    await routine.setMonthlyGoal('Prepare Prime sets');
    routine.refresh(Date.parse('2026-09-21T00:01:00Z'));
    await vi.waitFor(() => expect(routine.saving).toBe(false));
    expect(routine.state.periods.weekly.completed).toEqual([]);
    expect(routine.state.monthlyGoal).toBe('Prepare Prime sets');
  });

  it('resets only the elapsed cadence and preserves monthly text', async () => {
    const before = Date.parse('2026-09-30T23:59:00Z');
    const store = memoryStore();
    const routine = new RoutineController(store, before);
    await routine.toggle('login-tribute', true);
    routine.select('weekly');
    await routine.toggle('archon-hunt', true);
    routine.select('monthly');
    await routine.setMonthlyGoal('List three ranked mods');
    await routine.toggle('personal-goal', true);

    routine.refresh(Date.parse('2026-10-01T00:01:00Z'));
    await vi.waitFor(() => expect(routine.saving).toBe(false));

    expect(routine.state.periods.daily.completed).toEqual([]);
    expect(routine.state.periods.weekly.completed).toEqual(['archon-hunt']);
    expect(routine.state.periods.monthly.completed).toEqual([]);
    expect(routine.state.monthlyGoal).toBe('List three ranked mods');
  });

  it('reports a failed write and retries the latest rapid change serially', async () => {
    let releaseFirst!: () => void;
    let attempts = 0;
    const store = memoryStore(null, async () => {
      attempts++;
      if (attempts === 1) await new Promise<void>(resolve => { releaseFirst = resolve; });
      if (attempts <= 2) throw new Error('disk unavailable');
    });
    const routine = new RoutineController(store, Date.parse('2026-09-14T12:00:00Z'));
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

  it('does not carry completion from one monthly goal to its replacement', async () => {
    const routine = new RoutineController(memoryStore(), Date.parse('2026-09-14T12:00:00Z'));
    routine.select('monthly');
    await routine.setMonthlyGoal('List ranked mods');
    await routine.toggle('personal-goal', true);
    await routine.setMonthlyGoal('Prepare Prime sets');
    expect(routine.completed.size).toBe(0);
    expect(routine.tasks[0].title).toBe('Prepare Prime sets');
  });

  it('detects a browser adapter write that resolves without retaining data', async () => {
    const store = memoryStore();
    store.setSetting = async (_key, value) => { store.writes.push(value); };
    const routine = new RoutineController(store, Date.parse('2026-09-14T12:00:00Z'));
    await routine.toggle('sortie', true);
    expect(routine.saveError).toMatch(/Retry before closing/);
  });
});
