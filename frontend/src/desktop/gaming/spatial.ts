// Moving focus by direction, as a d-pad expects: to the nearest control on
// that side, preferring one in line with the current one.

export interface Box {
  readonly left: number;
  readonly top: number;
  readonly right: number;
  readonly bottom: number;
}

export type Direction = 'up' | 'down' | 'left' | 'right';

/** Index of the box to move to from `from`, or -1 when nothing lies that way. */
export function nearestBox(from: Box, boxes: readonly Box[], direction: Direction): number {
  const horizontal = direction === 'left' || direction === 'right';
  let best = -1;
  let bestScore = Infinity;
  boxes.forEach((box, index) => {
    // Distance along the move, edge to edge; a box must lie past the
    // current one's centre on that side.
    const gap =
      direction === 'right' ? box.left - from.right
      : direction === 'left' ? from.left - box.right
      : direction === 'down' ? box.top - from.bottom
      : from.top - box.bottom;
    const ahead =
      direction === 'right' ? (box.left + box.right) / 2 > (from.left + from.right) / 2 + 1
      : direction === 'left' ? (box.left + box.right) / 2 < (from.left + from.right) / 2 - 1
      : direction === 'down' ? (box.top + box.bottom) / 2 > (from.top + from.bottom) / 2 + 1
      : (box.top + box.bottom) / 2 < (from.top + from.bottom) / 2 - 1;
    if (!ahead) return;
    // Sideways distance: zero while the boxes overlap on the other axis.
    const offset = horizontal
      ? Math.max(0, box.top - from.bottom, from.top - box.bottom)
      : Math.max(0, box.left - from.right, from.left - box.right);
    const score = Math.max(0, gap) + offset * 3;
    if (score < bestScore) { bestScore = score; best = index; }
  });
  return best;
}
