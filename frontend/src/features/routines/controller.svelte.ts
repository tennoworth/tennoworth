import type { StateStore } from '../../contracts/state-store';

export type RoutineCadence = 'daily' | 'weekly' | 'monthly';

export interface RoutineTask {
  id: string;
  title: string;
  detail: string;
}

export const ROUTINE_TASKS: Record<'daily' | 'weekly', RoutineTask[]> = {
  daily: [
    { id: 'login-tribute', title: 'Claim the Daily Tribute', detail: 'Collect the login reward when you enter the game.' },
    { id: 'foundry-build', title: 'Keep the Foundry busy', detail: 'Start a Forma or another build you have planned.' },
    { id: 'syndicate-standing', title: 'Use Syndicate Standing', detail: 'Work toward the daily cap and choose an augment or arcane with demand.' },
    { id: 'steel-path-incursions', title: 'Check Steel Path Incursions', detail: 'Complete the missions that fit today’s play session.' },
    { id: 'sortie', title: 'Complete the Sortie', detail: 'Claim the current daily reward if the mission set fits your plan.' },
  ],
  weekly: [
    { id: 'maroo-treasure', title: 'Maroo’s Ayatan Treasure Hunt', detail: 'Claim the weekly sculpture and fill it when you need Endo.' },
    { id: 'archon-hunt', title: 'Archon Hunt', detail: 'Complete the weekly hunt for its reward and Archon Shard.' },
    { id: 'nightwave-acts', title: 'Review Nightwave acts', detail: 'Choose the acts that fit your goals before the week ends.' },
  ],
};

interface PeriodState {
  id: string;
  completed: string[];
}

interface PersistedRoutineState {
  version: 1;
  periods: Record<RoutineCadence, PeriodState>;
  monthlyGoal: string;
}

const EMPTY_GOAL = '';
const MAX_GOAL_LENGTH = 120;

export function routinePeriodId(cadence: RoutineCadence, timestamp: number): string {
  const date = new Date(timestamp);
  if (cadence === 'daily') return date.toISOString().slice(0, 10);
  if (cadence === 'monthly') return date.toISOString().slice(0, 7);
  const day = date.getUTCDay();
  const monday = new Date(Date.UTC(date.getUTCFullYear(), date.getUTCMonth(), date.getUTCDate() - ((day + 6) % 7)));
  return monday.toISOString().slice(0, 10);
}

export function nextRoutinePeriod(cadence: Exclude<RoutineCadence, 'monthly'>, timestamp: number): number {
  const date = new Date(timestamp);
  if (cadence === 'daily') return Date.UTC(date.getUTCFullYear(), date.getUTCMonth(), date.getUTCDate() + 1);
  const daysToMonday = ((8 - date.getUTCDay()) % 7) || 7;
  return Date.UTC(date.getUTCFullYear(), date.getUTCMonth(), date.getUTCDate() + daysToMonday);
}

function emptyState(now: number): PersistedRoutineState {
  return {
    version: 1,
    periods: {
      daily: { id: routinePeriodId('daily', now), completed: [] },
      weekly: { id: routinePeriodId('weekly', now), completed: [] },
      monthly: { id: routinePeriodId('monthly', now), completed: [] },
    },
    monthlyGoal: EMPTY_GOAL,
  };
}

function allowedTaskIds(cadence: RoutineCadence, monthlyGoal: string): Set<string> {
  return new Set(cadence === 'monthly' ? (monthlyGoal ? ['personal-goal'] : []) : ROUTINE_TASKS[cadence].map(task => task.id));
}

export function parseRoutineState(raw: string | null, now: number): { state: PersistedRoutineState; repaired: boolean } {
  const fallback = emptyState(now);
  if (!raw) return { state: fallback, repaired: false };
  try {
    const candidate = JSON.parse(raw) as Partial<PersistedRoutineState>;
    if (candidate.version !== 1 || !candidate.periods || typeof candidate.monthlyGoal !== 'string') {
      return { state: fallback, repaired: true };
    }
    const monthlyGoal = candidate.monthlyGoal.trim().slice(0, MAX_GOAL_LENGTH);
    const state = emptyState(now);
    state.monthlyGoal = monthlyGoal;
    for (const cadence of ['daily', 'weekly', 'monthly'] as const) {
      const source = candidate.periods[cadence];
      if (!source || source.id !== state.periods[cadence].id || !Array.isArray(source.completed)) continue;
      const allowed = allowedTaskIds(cadence, monthlyGoal);
      state.periods[cadence].completed = [...new Set(source.completed.filter((id): id is string => typeof id === 'string' && allowed.has(id)))];
    }
    return { state, repaired: JSON.stringify(state) !== raw };
  } catch {
    return { state: fallback, repaired: true };
  }
}

export class RoutineController {
  cadence = $state<RoutineCadence>('daily');
  now = $state(Date.now());
  state = $state<PersistedRoutineState>(emptyState(this.now));
  monthlyGoalDraft = $state('');
  saving = $state(false);
  saveError = $state('');
  #store: StateStore;
  #pending: string | null = null;
  #drain: Promise<void> | null = null;
  #repairOnStart = false;

  constructor(store: StateStore, now = Date.now()) {
    this.#store = store;
    this.now = now;
    const parsed = parseRoutineState(store.getSetting('routine-checklist'), now);
    this.state = parsed.state;
    this.monthlyGoalDraft = parsed.state.monthlyGoal;
    this.#repairOnStart = parsed.repaired;
  }

  get tasks(): RoutineTask[] {
    if (this.cadence !== 'monthly') return ROUTINE_TASKS[this.cadence];
    return this.state.monthlyGoal
      ? [{ id: 'personal-goal', title: this.state.monthlyGoal, detail: 'Your personal focus for this calendar month.' }]
      : [];
  }

  get completed(): Set<string> {
    return new Set(this.state.periods[this.cadence].completed);
  }

  start(): void {
    if (this.#repairOnStart) {
      this.#repairOnStart = false;
      void this.#save();
    }
  }

  refresh(now = Date.now()): void {
    this.now = now;
    let changed = false;
    for (const cadence of ['daily', 'weekly', 'monthly'] as const) {
      const id = routinePeriodId(cadence, now);
      if (this.state.periods[cadence].id !== id) {
        this.state.periods[cadence] = { id, completed: [] };
        changed = true;
      }
    }
    if (changed) void this.#save();
  }

  select(cadence: RoutineCadence): void {
    this.cadence = cadence;
    this.refresh(this.now);
  }

  toggle(taskId: string, checked: boolean): Promise<void> {
    const period = this.state.periods[this.cadence];
    const next = new Set(period.completed);
    if (checked) next.add(taskId); else next.delete(taskId);
    period.completed = [...next];
    return this.#save();
  }

  setMonthlyGoal(value: string): Promise<void> {
    const goal = value.trim().slice(0, MAX_GOAL_LENGTH);
    if (goal !== this.state.monthlyGoal) this.state.periods.monthly.completed = [];
    this.state.monthlyGoal = goal;
    this.monthlyGoalDraft = goal;
    return this.#save();
  }

  retry(): Promise<void> {
    return this.#save();
  }

  #save(): Promise<void> {
    this.#pending = JSON.stringify(this.state);
    if (!this.#drain) this.#drain = this.#drainWrites();
    return this.#drain;
  }

  async #drainWrites(): Promise<void> {
    this.saving = true;
    try {
      while (this.#pending) {
        const value = this.#pending;
        this.#pending = null;
        try {
          await this.#store.setSetting('routine-checklist', value);
          if (this.#store.getSetting('routine-checklist') !== value) throw new Error('write was not retained');
          this.saveError = '';
        } catch {
          this.saveError = 'Progress is only saved in this open view. Retry before closing the app.';
        }
      }
    } finally {
      this.saving = false;
      this.#drain = null;
    }
  }
}
