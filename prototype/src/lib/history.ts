// Year-long daily price history — the `history.json` the box builds from
// relics.run (see companion/wfm-scrape/src/history.rs for the pipeline side).
// Pure helpers over that shape; the transports load it, the UI renders it.

export interface HistorySeries {
  /** Daily median on the item's tracked tier; null = no closed trades (or the
   *  day's source file was unavailable — see History.missing_days). */
  median: Array<number | null>;
  volume: number[];
  subtype?: string | null;
}

export interface History {
  generated_at: string;
  /** ISO date of index 0. */
  start: string;
  days: number;
  /** ISO date of the last day with data (relics.run publishes with lag). */
  through?: string | null;
  items: Record<string, HistorySeries>;
  missing_days?: string[];
}

export function isHistory(h: unknown): h is History {
  const o = h as History | null;
  return !!o && typeof o === 'object' && typeof o.start === 'string' && typeof o.days === 'number' && !!o.items && typeof o.items === 'object';
}

/** ISO date for column `i`. */
export function dateAt(h: Pick<History, 'start'>, i: number): string {
  const t = Date.parse(`${h.start}T00:00:00Z`);
  return new Date(t + i * 86400000).toISOString().slice(0, 10);
}

/** Non-null (index, median) points, in order. */
export function points(series: HistorySeries): Array<[number, number]> {
  const out: Array<[number, number]> = [];
  series.median.forEach((m, i) => { if (m != null && Number.isFinite(m)) out.push([i, m]); });
  return out;
}

export interface YearStats {
  /** Latest daily median with data. */
  latest: number;
  /** Index of that day. */
  latestIdx: number;
  /** Median of the first ~30 days with data — the "a year ago" baseline. */
  baseline: number;
  /** Percent change latest vs baseline; null when either side is thin. */
  deltaPct: number | null;
  high: number;
  low: number;
  /** Days with at least one closed trade. */
  tradedDays: number;
}

/** Summary over the whole window. `null` when the series has under `minDays`
 *  traded days — a Δ1y from three trades is noise, not signal. */
export function yearStats(series: HistorySeries, minDays = 20): YearStats | null {
  const pts = points(series);
  if (pts.length < minDays) return null;
  const vals = pts.map((p) => p[1]);
  const [latestIdx, latest] = pts[pts.length - 1];
  const head = vals.slice(0, Math.min(30, Math.floor(vals.length / 3) || 1));
  const baseline = medianOf(head);
  const deltaPct = baseline > 0 && head.length >= 5 ? ((latest - baseline) / baseline) * 100 : null;
  return {
    latest,
    latestIdx,
    baseline,
    deltaPct,
    high: Math.max(...vals),
    low: Math.min(...vals),
    tradedDays: pts.length,
  };
}

function medianOf(xs: number[]): number {
  if (xs.length === 0) return 0;
  const s = [...xs].sort((a, b) => a - b);
  const mid = Math.floor(s.length / 2);
  return s.length % 2 ? s[mid] : (s[mid - 1] + s[mid]) / 2;
}

/** Downsample the daily series to `buckets` values (median of each bucket's
 *  non-null days; null buckets are skipped) — for a compact 1-year sparkline. */
export function weekly(series: HistorySeries, buckets = 52): number[] {
  const n = series.median.length;
  if (n === 0) return [];
  const size = Math.max(1, Math.ceil(n / buckets));
  const out: number[] = [];
  for (let i = 0; i < n; i += size) {
    const chunk = series.median.slice(i, i + size).filter((m): m is number => m != null && Number.isFinite(m));
    if (chunk.length) out.push(medianOf(chunk));
  }
  return out;
}
