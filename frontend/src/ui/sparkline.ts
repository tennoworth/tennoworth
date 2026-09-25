export interface SparklineGeometry {
  points: string;
  end: { x: number; y: number };
  // y of the reference line, or null when no baseline was given.
  baseline: number | null;
}

export function hasSparkline(arr: number[] | null | undefined): arr is number[] {
  return Array.isArray(arr) && arr.length >= 2;
}

// Normalise a series into the [inset, h-inset] band so a flat line still
// draws; the right edge is inset too so an endpoint mark is not clipped. The
// baseline joins the scale rather than being clamped, so how far the line sits
// above or below it is drawn to scale.
export function sparklineGeometry(
  arr: number[] | null | undefined,
  w: number,
  h: number,
  baseline: number | null = null,
  inset = 1,
): SparklineGeometry | null {
  if (!hasSparkline(arr)) return null;
  const hasBase = baseline != null && Number.isFinite(baseline);
  let min = hasBase ? baseline : Infinity;
  let max = hasBase ? baseline : -Infinity;
  for (const v of arr) {
    if (v < min) min = v;
    if (v > max) max = v;
  }
  const range = max - min || 1;
  const step = (w - inset) / (arr.length - 1);
  const y = (v: number) => (h - inset) - ((v - min) / range) * (h - 2 * inset);
  const coords = arr.map((v, i) => [i * step, y(v)] as const);
  const [ex, ey] = coords[coords.length - 1];
  return {
    points: coords.map(([px, py]) => `${px.toFixed(1)},${py.toFixed(1)}`).join(' '),
    end: { x: ex, y: ey },
    baseline: hasBase ? y(baseline) : null,
  };
}
