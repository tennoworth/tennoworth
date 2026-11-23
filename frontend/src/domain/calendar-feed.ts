// The dated things that move prices, in one list: when Baro lands and what he
// brings, when a Prime Vault rotation opens, what Darvo is discounting, and
// DE's dated events with fixed rewards. An unvaulting is the single most
// expensive surprise in prime trading, and knowing about these before they
// land is the entire value. The rule is native (market_domain::calendar::feed,
// documented there); this file is its call shape, and the shape the calendar
// views share with calendar-events.ts.

import type { CalendarHolding, FeedItem } from '../contracts/generated/web';
import type { Market, OwnedRecord } from '../contracts/data';
import { call } from './native';

export type { FeedText } from '../contracts/generated/web';
export type CalendarItem = FeedItem;

/** The snapshot surfaces the calendar rules read; the rest stays on the page. */
export function calendarMarket(market: Market | null | undefined): Pick<Market, 'baro' | 'calendar' | 'set_to_parts' | 'de' | 'event_rewards' | 'surface_provenance'> | null {
  if (!market) return null;
  const { baro, calendar, set_to_parts, de, event_rewards, surface_provenance } = market;
  return { baro, calendar, set_to_parts, de, event_rewards, surface_provenance };
}

/** Absent until an inventory loads, which is not the same as an empty one. */
export function holdings(owned: Map<string, OwnedRecord> | null | undefined): CalendarHolding[] | null {
  return owned ? [...owned.values()].map(({ slug, count }) => ({ slug, count })) : null;
}

/** Everything dated that has not finished, soonest first. */
export function buildCalendar(
  market: Market | null | undefined,
  owned: Map<string, OwnedRecord> | null | undefined,
  now: number,
): CalendarItem[] {
  return call({ op: 'calendar_feed', input: { market: calendarMarket(market), owned: holdings(owned), now } });
}
