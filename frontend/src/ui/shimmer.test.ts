import { describe, expect, it } from 'vitest';
import { PULSE_MS, pulseCenter, resolveFrame, shimmer, sparksAt, sweepCenter } from './shimmer';

describe('resolveFrame', () => {
  it('draws nothing when off, even mid-pass', () => {
    expect(resolveFrame('off', 3, 100, false)).toEqual({ kind: 'off' });
  });

  it('reduces every moving state to lit edges under reduced motion', () => {
    expect(resolveFrame('live', 3, null, true)).toEqual({ kind: 'settled' });
    expect(resolveFrame('settled', 3, 100, true)).toEqual({ kind: 'settled' });
  });

  it('runs one pass after a pulse, then shows the requested mode', () => {
    expect(resolveFrame('settled', 3, 0, false).kind).toBe('pulse');
    expect(resolveFrame('settled', 3, PULSE_MS - 1, false).kind).toBe('pulse');
    expect(resolveFrame('settled', 3, PULSE_MS, false)).toEqual({ kind: 'settled' });
    expect(resolveFrame('live', 3, PULSE_MS + 1, false)).toEqual({ kind: 'live', center: sweepCenter(3) });
  });
});

describe('light position', () => {
  it('keeps the wide light inside the field and returns every 28 seconds', () => {
    for (let t = 0; t < 60; t += 0.25) {
      const c = sweepCenter(t);
      expect(c).toBeGreaterThanOrEqual(0.12 - 1e-9);
      expect(c).toBeLessThanOrEqual(0.88 + 1e-9);
    }
    const period = (2 * Math.PI) / 0.2244;
    expect(sweepCenter(5 + period)).toBeCloseTo(sweepCenter(5), 9);
  });

  it('crosses the whole field once per pulse', () => {
    expect(pulseCenter(0)).toBeLessThan(0);
    expect(pulseCenter(PULSE_MS)).toBeGreaterThan(1);
    expect(pulseCenter(PULSE_MS / 2)).toBeGreaterThan(pulseCenter(PULSE_MS / 4));
  });
});

describe('sparksAt', () => {
  it('stays sparse and inside the field', () => {
    const w = 640;
    const h = 40;
    for (const t of [0, 7.5, 123.4]) {
      const sparks = sparksAt(w, h, t);
      expect(sparks.length).toBeLessThanOrEqual(Math.ceil(w / 96 + 2) * Math.ceil(h / 28 + 2));
      for (const s of sparks) {
        expect(s.x).toBeGreaterThanOrEqual(0);
        expect(s.x).toBeLessThanOrEqual(w);
        expect(s.y).toBeGreaterThanOrEqual(0);
        expect(s.y).toBeLessThanOrEqual(h);
        expect(s.alpha).toBeGreaterThanOrEqual(0);
        expect(s.alpha).toBeLessThanOrEqual(1);
      }
    }
    expect(sparksAt(w, h, 7.5)).toEqual(sparksAt(w, h, 7.5));
  });

  it('draws nothing in an empty field', () => {
    expect(sparksAt(0, 0, 4)).toEqual([]);
  });
});

describe('shimmer action', () => {
  it('adds nothing to a field that never shimmers', () => {
    const node = document.createElement('span');
    const action = shimmer(node, { mode: 'off' });
    action.update({ mode: 'off', pulse: 1 });
    expect(node.querySelector('canvas')).toBeNull();
    action.destroy();
  });

  it('stays inert where canvas drawing is unavailable', () => {
    const node = document.createElement('span');
    const action = shimmer(node, { mode: 'live', sparks: true });
    expect(node.querySelector('canvas')).toBeNull();
    action.destroy();
  });
});
