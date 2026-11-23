// Year-long daily price history - the `history.json` the box builds from
// relics.run (see rust/wfm-scrape/src/pipeline/history.rs for the pipeline side).
// The transports load it; the summaries are native (market_domain::advisor).

import type { YearView } from '../contracts/generated/web';
import { call } from './native';

export interface HistorySeries {
  /** Daily median on the item's tracked tier; null = no closed trades (or the
   *  day's source file was unavailable - see History.missing_days). */
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

/** For each series: its summary over the whole window (`stats`, null under
 *  `minDays` traded days - a Δ1y from three trades is noise, not signal) and
 *  its daily medians downsampled to `buckets` values for a 1-year sparkline. */
export function yearViews(series: HistorySeries[], { minDays, buckets }: { minDays?: number; buckets?: number } = {}): YearView[] {
  if (series.length === 0) return [];
  return call({ op: 'year_views', input: {
    medians: series.map((s) => s.median.map((m) => typeof m === 'number' ? m : null)),
    ...(minDays === undefined ? {} : { min_days: minDays }),
    ...(buckets === undefined ? {} : { buckets }),
  } });
}
