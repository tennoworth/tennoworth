// Every dated thing the Calendar shows, from data the app already has.
//
// Confirmed dates come from DE's worldState (through market.json) and from the
// curated announcement feed. Estimates (Baro's later visits, Prime vaulting)
// are projections and say so: they carry `estimate` and never advice, because
// presenting a guess as a date is how a calendar loses trust. The rows come
// from the native rule (market_domain::calendar::events); this file names the
// sets they carry and orders them, and the routine rows (desktop only) join
// them here.

import type { Market, OwnedRecord } from '../contracts/data';
import type { AnnouncementFeed } from '../contracts/generated/desktop';
import type { DatedEvent, DatedSource, DatedText } from '../contracts/generated/web';
import { calendarMarket, holdings } from './calendar-feed';
import { call } from './native';

export type EventKind = DatedEvent['kind'] | 'routine';

export interface CalendarEvent {
  /** Stable across refreshes of the same snapshot, so a selection survives. */
  id: string;
  kind: EventKind;
  /** Epoch ms. */
  from: number;
  /** Epoch ms; `null` for a single moment such as a deadline. */
  to: number | null;
  /** What to say about it; the view words it (features/calendar/wording.ts). */
  text: CalendarText;
  estimate: boolean;
  /** Who published the date. */
  source: DatedSource | 'routine';
  sourceUrl?: string;
  /** Owned slugs this event touches; absent when that cannot be known. */
  affects?: string[];
}

/**
 * The facts behind an event's title and description. Events DE or our feed
 * name themselves carry that text as published (`given`); the rest are worded
 * by the view, in the reader's language.
 */
export type CalendarText =
  | Exclude<DatedText, { kind: 'resurgence' | 'vault' }>
  | { kind: 'resurgence' | 'vault'; names: string[] }
  /** A routine list's next reset and what is still open on it (desktop only). */
  | { kind: 'routine'; cadence: 'daily' | 'weekly' | 'monthly'; open: string[]; total: number };

export function primeName(market: Market | null | undefined, slug: string): string {
  return market?.calendar?.primes?.[slug]?.name
    ?? slug.replace(/_set$/, '').split('_').map(word => word.charAt(0).toUpperCase() + word.slice(1)).join(' ');
}

export function joinNames(names: string[]): string {
  return names.length > 1 ? `${names.slice(0, -1).join(', ')} and ${names.at(-1)}` : names[0] ?? '';
}

function named(market: Market | null | undefined, text: DatedText): CalendarText {
  if (text.kind === 'resurgence') return { kind: 'resurgence', names: text.sets.map(slug => primeName(market, slug)) };
  if (text.kind === 'vault') return { kind: 'vault', names: text.sets.map(slug => primeName(market, slug)).sort() };
  return text;
}

export function calendarEvents(
  market: Market | null | undefined,
  feed: AnnouncementFeed | null | undefined,
  now: number,
  owned?: Map<string, OwnedRecord> | null,
): CalendarEvent[] {
  const events = call({ op: 'calendar_events', input: { market: calendarMarket(market), feed: feed ?? null, owned: holdings(owned), now } });
  return events
    .map(({ text, ...event }): CalendarEvent => ({ ...event, text: named(market, text) }))
    .sort((a, b) => a.from - b.from || a.id.localeCompare(b.id));
}

/** The moment an event matters most from `now`: its start, or its end once under way. */
export function eventKey(event: CalendarEvent, now: number): number {
  return event.from <= now && event.to !== null ? event.to : event.from;
}

export function eventEnd(event: CalendarEvent): number {
  return event.to ?? event.from;
}
