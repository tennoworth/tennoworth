import type { StateStore } from '../../contracts/state-store';

export type RoutineCadence = 'daily' | 'weekly' | 'monthly';

export interface RoutineTask {
  id: string;
  title: string;
  detail: string;
}

export interface MonthlyGoal {
  id: string;
  text: string;
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
  version: 2;
  periods: Record<RoutineCadence, PeriodState>;
  monthlyGoals: MonthlyGoal[];
  nextGoalId: number;
}

export const MAX_GOAL_LENGTH = 120;
export const MAX_GOALS = 8;

const GOAL_DETAIL = 'Your personal focus for this calendar month.';
const NUMBERED_GOAL_ID = /^goal-(\d+)$/;

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
    version: 2,
    periods: {
      daily: { id: routinePeriodId('daily', now), completed: [] },
      weekly: { id: routinePeriodId('weekly', now), completed: [] },
      monthly: { id: routinePeriodId('monthly', now), completed: [] },
    },
    monthlyGoals: [],
    nextGoalId: 1,
  };
}

function goalText(value: unknown): string {
  return typeof value === 'string' ? value.trim().slice(0, MAX_GOAL_LENGTH) : '';
}

// Ids the payload mentioned are never handed out again, even when the entry
// that carried them was dropped - a stale nextGoalId would otherwise reuse one
// and silently merge two goals into the same completion row.
function normalizeGoals(raw: unknown, rawNextId: unknown): { goals: MonthlyGoal[]; nextGoalId: number } {
  const goals: MonthlyGoal[] = [];
  const seen = new Set<string>();
  let highest = 0;
  if (Array.isArray(raw)) {
    for (const entry of raw) {
      if (!entry || typeof entry !== 'object') continue;
      const { id, text } = entry as Partial<MonthlyGoal>;
      if (typeof id !== 'string' || !id) continue;
      const numbered = NUMBERED_GOAL_ID.exec(id);
      if (numbered) highest = Math.max(highest, Number(numbered[1]));
      if (seen.has(id) || goals.length === MAX_GOALS) continue;
      const value = goalText(text);
      if (!value) continue;
      seen.add(id);
      goals.push({ id, text: value });
    }
  }
  const candidate = typeof rawNextId === 'number' && Number.isInteger(rawNextId) && rawNextId > 0 ? rawNextId : 0;
  return { goals, nextGoalId: Math.max(highest + 1, candidate, 1) };
}

export function parseRoutineState(raw: string | null, now: number): { state: PersistedRoutineState; repaired: boolean } {
  const fallback = emptyState(now);
  if (!raw) return { state: fallback, repaired: false };
  try {
    const candidate = JSON.parse(raw) as Partial<PersistedRoutineState>;
    if (candidate.version !== 2 || !candidate.periods || !Array.isArray(candidate.monthlyGoals)) {
      return { state: fallback, repaired: true };
    }
    const state = emptyState(now);
    const { goals, nextGoalId } = normalizeGoals(candidate.monthlyGoals, candidate.nextGoalId);
    state.monthlyGoals = goals;
    state.nextGoalId = nextGoalId;
    const allowed: Record<RoutineCadence, Set<string>> = {
      daily: new Set(ROUTINE_TASKS.daily.map(task => task.id)),
      weekly: new Set(ROUTINE_TASKS.weekly.map(task => task.id)),
      monthly: new Set(goals.map(goal => goal.id)),
    };
    for (const cadence of ['daily', 'weekly', 'monthly'] as const) {
      const source = candidate.periods[cadence];
      if (!source || source.id !== state.periods[cadence].id || !Array.isArray(source.completed)) continue;
      state.periods[cadence].completed = [...new Set(source.completed.filter((id): id is string => typeof id === 'string' && allowed[cadence].has(id)))];
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
    this.#repairOnStart = parsed.repaired;
  }

  get tasks(): RoutineTask[] {
    if (this.cadence !== 'monthly') return ROUTINE_TASKS[this.cadence];
    return this.state.monthlyGoals.map(goal => ({ id: goal.id, title: goal.text, detail: GOAL_DETAIL }));
  }

  get goalLimitReached(): boolean {
    return this.state.monthlyGoals.length >= MAX_GOALS;
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

  addMonthlyGoal(value: string): Promise<void> {
    const text = goalText(value);
    if (!text || this.goalLimitReached) return Promise.resolve();
    this.state.monthlyGoals.push({ id: `goal-${this.state.nextGoalId}`, text });
    this.state.nextGoalId += 1;
    this.monthlyGoalDraft = '';
    return this.#save();
  }

  renameMonthlyGoal(id: string, value: string): Promise<void> {
    const goal = this.state.monthlyGoals.find(entry => entry.id === id);
    const text = goalText(value);
    if (!goal || !text || text === goal.text) return Promise.resolve();
    goal.text = text;
    this.#forgetCompletion(id);
    return this.#save();
  }

  removeMonthlyGoal(id: string): Promise<void> {
    const index = this.state.monthlyGoals.findIndex(entry => entry.id === id);
    if (index === -1) return Promise.resolve();
    this.state.monthlyGoals.splice(index, 1);
    this.#forgetCompletion(id);
    return this.#save();
  }

  retry(): Promise<void> {
    return this.#save();
  }

  #forgetCompletion(id: string): void {
    const period = this.state.periods.monthly;
    if (period.completed.includes(id)) period.completed = period.completed.filter(entry => entry !== id);
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
