import { describe, expect, it } from 'vitest';
import type { SettingsStore } from '../contracts/state-store';
import { GLOBAL_GAP_DAYS, promptDue, readPromptState, withAsk, withDone, withLaunch, type PromptPolicy, type PromptState } from './prompts';
import { PromptSession } from './prompt-session.svelte';

const DAY = 86_400_000;
const T0 = Date.UTC(2026, 9, 1);
const policy = (id: string, more: Partial<PromptPolicy> = {}): PromptPolicy => ({ id, delayDays: 0, minLaunches: 1, gapDays: 14, maxAsks: 3, ...more });
const launched = (launches = 1): PromptState => ({ first_seen: T0, launches, prompts: {} });
function memory(initial: string | null = null) {
  const values = new Map<string, string>(initial === null ? [] : [['prompts', initial]]);
  const store: SettingsStore = { mode: 'local', hydrate: async () => {}, getSetting: key => values.get(key) ?? null, setSetting: async (key, value) => { values.set(key, value); } };
  return { store, values };
}

describe('prompt schedule', () => {
  it('waits for first use, the delay and the launch minimum', () => {
    const p = policy('a', { delayDays: 14, minLaunches: 5 });
    expect(promptDue({ launches: 9, prompts: {} }, p, T0 + 99 * DAY)).toBe(false);
    expect(promptDue(launched(5), p, T0 + 13 * DAY)).toBe(false);
    expect(promptDue(launched(4), p, T0 + 14 * DAY)).toBe(false);
    expect(promptDue(launched(5), p, T0 + 14 * DAY)).toBe(true);
  });

  it('spaces asks by the prompt gap and retires after the last one', () => {
    const p = policy('a', { gapDays: 14, maxAsks: 2 });
    let s = withAsk(launched(), 'a', T0);
    expect(promptDue(s, p, T0 + 13 * DAY)).toBe(false);
    expect(promptDue(s, p, T0 + 14 * DAY)).toBe(true);
    s = withAsk(s, 'a', T0 + 14 * DAY);
    expect(promptDue(s, p, T0 + 400 * DAY)).toBe(false);
  });

  it('keeps a week between asks of different prompts', () => {
    const s = withAsk(launched(), 'a', T0);
    expect(promptDue(s, policy('b'), T0 + (GLOBAL_GAP_DAYS - 1) * DAY)).toBe(false);
    expect(promptDue(s, policy('b'), T0 + GLOBAL_GAP_DAYS * DAY)).toBe(true);
  });

  it('never asks again once accepted', () => {
    expect(promptDue(withDone(launched(), 'a'), policy('a', { gapDays: 0 }), T0 + 400 * DAY)).toBe(false);
  });

  it('records the first launch once and counts the rest', () => {
    const s = withLaunch(withLaunch({ launches: 0, prompts: {} }, T0), T0 + DAY);
    expect(s).toMatchObject({ first_seen: T0, launches: 2 });
  });

  it('starts over from an unreadable value instead of throwing', () => {
    for (const raw of ['{not json', '[]', 'null', '{"launches":"x","prompts":{"a":7}}']) {
      expect(readPromptState(memory(raw).store)).toEqual({ launches: 0, prompts: {} });
    }
    expect(readPromptState(memory('{"first_seen":5,"launches":2,"prompts":{"a":{"asks":1,"done":true}}}').store))
      .toEqual({ first_seen: 5, launches: 2, prompts: { a: { asks: 1, done: true } } });
  });
});

describe('prompt session', () => {
  it('asks at most one prompt per launch and persists the ask', () => {
    const { store, values } = memory();
    const s = new PromptSession(store, [policy('a'), policy('b')], () => T0);
    s.launch(); s.notes('none');
    s.claim('b');
    expect(s.current).toBeNull();
    s.claim('a');
    expect(s.current).toBe('a');
    s.claim('b');
    expect(s.current).toBe('a');
    expect(JSON.parse(values.get('prompts')!)).toMatchObject({ first_seen: T0, launches: 1, last_ask: T0, prompts: { a: { asks: 1 } } });
  });

  it('lets a lower prompt through once a higher one passes or is not due', () => {
    const passed = new PromptSession(memory().store, [policy('a'), policy('b')], () => T0);
    passed.launch(); passed.notes('none'); passed.claim('b'); passed.pass('a');
    expect(passed.current).toBe('b');
    const notDue = new PromptSession(memory().store, [policy('a', { delayDays: 30 }), policy('b')], () => T0);
    notDue.launch(); notDue.notes('none'); notDue.claim('b');
    expect(notDue.current).toBe('b');
  });

  it('asks nothing until this launch has an inventory, even for a returning user', () => {
    const { store, values } = memory(JSON.stringify(launched(3)));
    const s = new PromptSession(store, [policy('a')], () => T0 + DAY);
    s.claim('a');
    expect(s.current).toBeNull();
    s.launch(); s.notes('none');
    s.launch(); s.notes('none');
    expect(s.current).toBe('a');
    expect(JSON.parse(values.get('prompts')!).launches).toBe(4);
  });

  it('does not ask the next launch inside the gap, and Not now only hides', () => {
    const { store } = memory();
    const first = new PromptSession(store, [policy('a')], () => T0);
    first.launch(); first.notes('none'); first.claim('a'); first.decline();
    expect(first.hidden).toBe(true);
    const next = new PromptSession(store, [policy('a')], () => T0 + DAY);
    next.launch(); next.notes('none'); next.claim('a');
    expect(next.current).toBeNull();
    const later = new PromptSession(store, [policy('a')], () => T0 + 14 * DAY);
    later.launch(); later.notes('none'); later.claim('a');
    expect(later.current).toBe('a');
  });

  it('asks nothing while this launch’s release notes are unknown or showing', () => {
    const s = new PromptSession(memory().store, [policy('a')], () => T0);
    s.launch(); s.claim('a');
    expect(s.current).toBeNull();
    s.notes('pending');
    expect(s.current).toBeNull();
    s.notes('dismissed');
    expect(s.current).toBe('a');
  });

  it('counts a launch as an update only once its notes are dismissed', () => {
    const update = policy('u', { firstOnUpdate: true });
    const plain = new PromptSession(memory().store, [update], () => T0);
    plain.launch(); plain.claim('u'); plain.notes('none');
    expect(plain.current).toBeNull();
    const updated = new PromptSession(memory().store, [update], () => T0);
    updated.launch(); updated.claim('u'); updated.notes('pending'); updated.notes('dismissed');
    expect(updated.current).toBe('u');
    expect(promptDue(withAsk(launched(), 'u', T0), { ...update, gapDays: 0 }, T0 + GLOBAL_GAP_DAYS * DAY)).toBe(true);
  });
});
