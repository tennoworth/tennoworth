// Controller input for the Gaming Mode layout. The webview's Gamepad API
// reports a controller in the W3C "standard" layout, which is what Steam Input
// presents for a Steam Deck or any pad it manages.

export type PadAction =
  | 'up' | 'down' | 'left' | 'right'
  | 'accept' | 'back' | 'secondary' | 'scan' | 'prev' | 'next' | 'menu';

export const DIRECTIONS: readonly PadAction[] = ['up', 'down', 'left', 'right'];

const BUTTONS: ReadonlyArray<readonly [number, PadAction]> = [
  [0, 'accept'], [1, 'back'], [2, 'secondary'], [3, 'scan'],
  [4, 'prev'], [5, 'next'], [9, 'menu'],
  [12, 'up'], [13, 'down'], [14, 'left'], [15, 'right'],
];

/** How far the left stick must lean to count as a direction. */
const STICK = 0.5;

export interface PadState {
  readonly buttons: ReadonlyArray<{ readonly pressed: boolean }>;
  readonly axes: readonly number[];
}

/** The actions a pad is holding right now. */
export function heldActions(pad: PadState): Set<PadAction> {
  const held = new Set<PadAction>();
  for (const [index, action] of BUTTONS) if (pad.buttons[index]?.pressed) held.add(action);
  const [x = 0, y = 0] = pad.axes;
  if (Math.abs(x) >= STICK && Math.abs(x) >= Math.abs(y)) held.add(x < 0 ? 'left' : 'right');
  else if (Math.abs(y) >= STICK) held.add(y < 0 ? 'up' : 'down');
  return held;
}

/** First repeat of a held direction, then the pace after it. */
export const REPEAT_DELAY_MS = 380;
export const REPEAT_EVERY_MS = 110;

/**
 * Turns successive held sets into actions: every button once on press, and a
 * held direction again after a delay, so a list can be walked by holding the
 * d-pad as with a keyboard's auto-repeat.
 */
export class PadReader {
  private held = new Set<PadAction>();
  private nextRepeat = new Map<PadAction, number>();

  step(now: Set<PadAction>, at: number): PadAction[] {
    const fired: PadAction[] = [];
    for (const action of now) {
      if (!this.held.has(action)) {
        fired.push(action);
        if (DIRECTIONS.includes(action)) this.nextRepeat.set(action, at + REPEAT_DELAY_MS);
      } else if (DIRECTIONS.includes(action)) {
        const due = this.nextRepeat.get(action) ?? Infinity;
        if (at >= due) {
          fired.push(action);
          this.nextRepeat.set(action, at + REPEAT_EVERY_MS);
        }
      }
    }
    for (const action of this.held) if (!now.has(action)) this.nextRepeat.delete(action);
    this.held = now;
    return fired;
  }
}

/** The same actions from a keyboard, for a Steam Input keyboard layout or a desk. */
export function keyAction(key: string): PadAction | null {
  switch (key) {
    case 'ArrowUp': return 'up';
    case 'ArrowDown': return 'down';
    case 'ArrowLeft': return 'left';
    case 'ArrowRight': return 'right';
    case 'Enter': return 'accept';
    case 'Escape': case 'Backspace': return 'back';
    case 'PageUp': return 'prev';
    case 'PageDown': return 'next';
    default: return null;
  }
}
