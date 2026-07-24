// Normalise a point series into a fixed [1, h-1] band so a flat line still
// draws, returning SVG <polyline points="..."> coordinates. Shared by
// ResultsTable.svelte and MarketBrowser.svelte's 7-day-median sparklines —
// each passes its own viewBox dimensions.
export function sparklinePoints(arr: number[] | null | undefined, w: number, h: number): string | null {
  if (!Array.isArray(arr) || arr.length < 2) return null;
  let min = Infinity, max = -Infinity;
  for (const v of arr) {
    if (v < min) min = v;
    if (v > max) max = v;
  }
  const range = max - min || 1;
  const step = w / (arr.length - 1);
  return arr.map((v, i) => {
    const x = i * step;
    const y = (h - 1) - ((v - min) / range) * (h - 2);
    return `${x.toFixed(1)},${y.toFixed(1)}`;
  }).join(' ');
}
