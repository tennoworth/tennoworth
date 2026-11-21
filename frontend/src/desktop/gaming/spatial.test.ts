import { describe, expect, it } from 'vitest';
import { nearestBox, type Box } from './spatial';

const box = (left: number, top: number, width = 100, height = 40): Box => ({ left, top, right: left + width, bottom: top + height });

describe('nearestBox', () => {
  // A list of rows, each with a button at its right end.
  const rows = [box(0, 0, 400), box(0, 50, 400), box(0, 100, 400)];
  const buttons = [box(420, 0), box(420, 50), box(420, 100)];
  const all = [...rows, ...buttons];

  it('moves down a list to the next row, not a farther one', () => {
    expect(nearestBox(rows[0], all, 'down')).toBe(1);
  });
  it('moves right to the button on the same row', () => {
    expect(nearestBox(rows[1], all, 'right')).toBe(4);
  });
  it('moves left back to the row the button sits on', () => {
    expect(nearestBox(buttons[2], all, 'left')).toBe(2);
  });
  it('finds nothing past the edge', () => {
    expect(nearestBox(rows[0], all, 'up')).toBe(-1);
    expect(nearestBox(buttons[0], all, 'right')).toBe(-1);
  });
  it('prefers a control in line over a nearer one off to the side', () => {
    const tabs = [box(0, 0), box(110, 0), box(220, 0)];
    const below = [box(230, 60), box(0, 200)];
    expect(nearestBox(tabs[2], [...tabs, ...below], 'down')).toBe(3);
    expect(nearestBox(tabs[0], [...tabs, ...below], 'down')).toBe(4);
  });
});
