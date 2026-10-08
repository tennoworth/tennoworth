import type { SettingsStore } from '../contracts/state-store';

const DAY = 86_400_000;
/** Between any two asks, whichever prompts they belong to. */
export const GLOBAL_GAP_DAYS = 7;

export interface PromptPolicy {
  /** Permanent: a new id is a new prompt, asked again from scratch. */
  id: string;
  /** Days after the first launch with an inventory before the first ask. */
  delayDays: number;
  /** Launches with an inventory, this one included, before the first ask. */
  minLaunches: number;
  gapDays: number;
  /** After this many asks an unanswered prompt retires. */
  maxAsks: number;
  /** The first ask waits for a launch that installed an update, once its
   *  release notes are out of the way. */
  firstOnUpdate?: boolean;
}

interface PromptRecord { asks: number; last_ask?: number; done?: boolean }
export interface PromptState {
  first_seen?: number;
  launches: number;
  last_ask?: number;
  prompts: Record<string, PromptRecord>;
}

const finite = (value: unknown): value is number => typeof value === 'number' && Number.isFinite(value);

// An unreadable value starts over rather than throwing: that can re-ask, but a
// broken setting must never break the shell.
export function readPromptState(store: SettingsStore): PromptState {
  const state: PromptState = { launches: 0, prompts: {} };
  try {
    const raw: unknown = JSON.parse(store.getSetting('prompts') ?? '{}');
    if (!raw || typeof raw !== 'object') return state;
    const value = raw as Record<string, unknown>;
    if (finite(value.first_seen)) state.first_seen = value.first_seen;
    if (finite(value.launches)) state.launches = value.launches;
    if (finite(value.last_ask)) state.last_ask = value.last_ask;
    if (value.prompts && typeof value.prompts === 'object') {
      for (const [id, entry] of Object.entries(value.prompts as Record<string, unknown>)) {
        if (!entry || typeof entry !== 'object') continue;
        const e = entry as Record<string, unknown>;
        state.prompts[id] = { asks: finite(e.asks) ? e.asks : 0, ...(finite(e.last_ask) ? { last_ask: e.last_ask } : {}), ...(e.done === true ? { done: true } : {}) };
      }
    }
  } catch { /* start over */ }
  return state;
}

export function promptDue(state: PromptState, policy: PromptPolicy, now: number, updated = false): boolean {
  const record = state.prompts[policy.id] ?? { asks: 0 };
  if (record.done || record.asks >= policy.maxAsks) return false;
  if (policy.firstOnUpdate && record.asks === 0 && !updated) return false;
  if (state.first_seen === undefined || now - state.first_seen < policy.delayDays * DAY) return false;
  if (state.launches < policy.minLaunches) return false;
  if (state.last_ask !== undefined && now - state.last_ask < GLOBAL_GAP_DAYS * DAY) return false;
  return record.last_ask === undefined || now - record.last_ask >= policy.gapDays * DAY;
}

export const withLaunch = (state: PromptState, now: number): PromptState =>
  ({ ...state, first_seen: state.first_seen ?? now, launches: state.launches + 1 });

export function withAsk(state: PromptState, id: string, now: number): PromptState {
  const record = state.prompts[id] ?? { asks: 0 };
  return { ...state, last_ask: now, prompts: { ...state.prompts, [id]: { ...record, asks: record.asks + 1, last_ask: now } } };
}

export function withDone(state: PromptState, id: string): PromptState {
  const record = state.prompts[id] ?? { asks: 0 };
  return { ...state, prompts: { ...state.prompts, [id]: { ...record, done: true } } };
}
