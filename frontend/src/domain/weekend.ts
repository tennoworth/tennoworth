// The weekend nudge is native (market_domain::calendar::weekend_nudge,
// documented there); the page supplies the weekday, since only it knows the
// reader's time zone.
import { native } from './native';

export type WeekendNudge = 'before' | 'during';

const NUDGES = [null, 'before', 'during'] as const;

export function weekendNudge(now: Date): WeekendNudge | null {
  return NUDGES[native().web_weekend_nudge(now.getDay())] ?? null;
}
