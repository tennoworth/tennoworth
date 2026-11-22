import { describe, expect, it } from 'vitest';
import { heldActions, keyAction, PadReader, REPEAT_DELAY_MS, REPEAT_EVERY_MS, type PadAction } from './pad';

const pad = (pressed: number[], axes: number[] = [0, 0]) => ({
  buttons: Array.from({ length: 17 }, (_, i) => ({ pressed: pressed.includes(i) })),
  axes,
});

describe('heldActions', () => {
  it('reads the standard layout', () => {
    expect([...heldActions(pad([0, 4, 12]))].sort()).toEqual(['accept', 'prev', 'up']);
    expect([...heldActions(pad([1, 3, 5, 9]))].sort()).toEqual(['back', 'menu', 'next', 'scan']);
  });
  it('takes the stick as a direction once it leans far enough, along its stronger axis', () => {
    expect([...heldActions(pad([], [0.3, 0]))]).toEqual([]);
    expect([...heldActions(pad([], [0.9, 0.6]))]).toEqual(['right']);
    expect([...heldActions(pad([], [0.2, -0.8]))]).toEqual(['up']);
  });
});

describe('PadReader', () => {
  const held = (...actions: PadAction[]) => new Set(actions);
  it('fires a button once per press', () => {
    const reader = new PadReader();
    expect(reader.step(held('accept'), 0)).toEqual(['accept']);
    expect(reader.step(held('accept'), 2000)).toEqual([]);
    expect(reader.step(held(), 2100)).toEqual([]);
    expect(reader.step(held('accept'), 2200)).toEqual(['accept']);
  });
  it('repeats a held direction after a delay, then steadily', () => {
    const reader = new PadReader();
    expect(reader.step(held('down'), 0)).toEqual(['down']);
    expect(reader.step(held('down'), REPEAT_DELAY_MS - 1)).toEqual([]);
    expect(reader.step(held('down'), REPEAT_DELAY_MS)).toEqual(['down']);
    expect(reader.step(held('down'), REPEAT_DELAY_MS + REPEAT_EVERY_MS - 1)).toEqual([]);
    expect(reader.step(held('down'), REPEAT_DELAY_MS + REPEAT_EVERY_MS)).toEqual(['down']);
  });
  it('restarts the delay after a release', () => {
    const reader = new PadReader();
    reader.step(held('left'), 0);
    reader.step(held(), 100);
    expect(reader.step(held('left'), 120)).toEqual(['left']);
    expect(reader.step(held('left'), 120 + REPEAT_DELAY_MS - 1)).toEqual([]);
  });
});

describe('keyAction', () => {
  it('maps the keys a Steam Input keyboard layout sends', () => {
    expect(keyAction('ArrowDown')).toBe('down');
    expect(keyAction('Enter')).toBe('accept');
    expect(keyAction('Escape')).toBe('back');
    expect(keyAction('PageDown')).toBe('next');
    expect(keyAction('a')).toBeNull();
  });
});
