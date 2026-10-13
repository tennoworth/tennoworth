// Every dated thing the Calendar shows, from data the app already has.
//
// Confirmed dates come from DE's worldState (through market.json) and from the
// curated announcement feed. Estimates (Baro's later visits, Prime vaulting)
// are projections and say so: they carry `estimate` and never advice, because
// presenting a guess as a date is how a calendar loses trust.

import type { Market, OwnedRecord, ResurgenceRotation } from '../contracts/data';
import type { AnnouncementFeed } from '../contracts/generated/desktop';

export type EventKind = 'event' | 'baro' | 'resurgence' | 'vault' | 'notice' | 'deal';

export interface CalendarEvent {
  /** Stable across refreshes of the same snapshot, so a selection survives. */
  id: string;
  kind: EventKind;
  /** Epoch ms. */
  from: number;
  /** Epoch ms; `null` for a single moment such as a deadline. */
  to: number | null;
  title: string;
  /** Short label for the month grid and lanes. */
  short: string;
  /** What happens, in one or two sentences. */
  detail: string;
  estimate: boolean;
  /** Who published the date. */
  source: 'worldstate' | 'estimate' | 'notice';
  sourceUrl?: string;
  /** Owned slugs this event touches; absent when that cannot be known. */
  affects?: string[];
}

const DAY = 86_400_000;
const BARO_PERIOD = 14 * DAY;
/** Projected Baro visits shown after the confirmed one. */
const BARO_PROJECTIONS = 2;
/** How far ahead estimated vaulting is worth showing. */
const VAULT_HORIZON = 120 * DAY;
/** Finished events stay visible this long, so the month view keeps context. */
const PAST = 45 * DAY;

const at = (value: string | undefined): number => (value ? Date.parse(value) : NaN);

export function primeName(market: Market, slug: string): string {
  return market.calendar?.primes?.[slug]?.name
    ?? slug.replace(/_set$/, '').split('_').map(word => word.charAt(0).toUpperCase() + word.slice(1)).join(' ');
}

export function joinNames(names: string[]): string {
  return names.length > 1 ? `${names.slice(0, -1).join(', ')} and ${names.at(-1)}` : names[0] ?? '';
}

function heldParts(market: Market, sets: string[], owned: Map<string, OwnedRecord> | null | undefined): string[] | undefined {
  if (!owned) return undefined;
  const held = new Set<string>();
  for (const record of owned.values()) if (record.count > 0) held.add(record.slug);
  const hits: string[] = [];
  for (const set of sets) {
    if (held.has(set)) hits.push(set);
    for (const part of market.set_to_parts?.[set]?.parts ?? []) if (held.has(part.slug)) hits.push(part.slug);
  }
  return [...new Set(hits)];
}

function baro(market: Market, now: number): CalendarEvent[] {
  const visit = market.baro;
  const from = at(visit?.activation);
  const to = at(visit?.expiry);
  if (!visit || !Number.isFinite(from) || !Number.isFinite(to) || to < now - PAST) return [];
  const where = visit.location || 'a relay';
  const events: CalendarEvent[] = [{
    id: `baro-${from}`, kind: 'baro', from, to, estimate: false, source: 'worldstate',
    title: `Baro Ki'Teer at ${where}`, short: `Baro · ${where}`,
    detail: 'He sells for 48 hours. Items he brings usually fall in price while he is here.',
  }];
  for (let n = 1; n <= BARO_PROJECTIONS; n++) {
    const start = from + n * BARO_PERIOD;
    if (start + (to - from) < now) continue;
    events.push({
      id: `baro-est-${start}`, kind: 'baro', from: start, to: start + (to - from), estimate: true, source: 'estimate',
      title: "Baro Ki'Teer, expected", short: 'Baro (est.)',
      detail: 'He visits every 14 days. The relay and his stock are unknown until DE publishes the visit.',
    });
  }
  return events;
}

function resurgence(market: Market, now: number, owned: Map<string, OwnedRecord> | null | undefined): CalendarEvent[] {
  const rotations: ResurgenceRotation[] = [
    ...(market.calendar?.resurgence ?? []),
    ...(market.calendar?.resurgence_current ? [market.calendar.resurgence_current] : []),
  ];
  const seen = new Set<string>();
  const events: CalendarEvent[] = [];
  for (const rotation of rotations) {
    const from = at(rotation.from);
    const to = at(rotation.to);
    if (!Number.isFinite(from) || !Number.isFinite(to) || to < now - PAST || !rotation.frames.length) continue;
    const id = `resurgence-${from}`;
    if (seen.has(id)) continue;
    seen.add(id);
    const names = joinNames(rotation.frames.map(slug => primeName(market, slug)));
    events.push({
      id, kind: 'resurgence', from, to, estimate: false, source: 'worldstate',
      title: `Prime Resurgence: ${names}`, short: `Resurgence · ${names}`,
      detail: `Varzia sells ${names} relics. Fresh supply tends to keep their parts cheap until the rotation ends; prices usually recover afterwards.`,
      affects: heldParts(market, rotation.frames, owned),
    });
  }
  return events;
}

function vaulting(market: Market, now: number, owned: Map<string, OwnedRecord> | null | undefined): CalendarEvent[] {
  const byDate = new Map<number, string[]>();
  for (const [slug, prime] of Object.entries(market.calendar?.primes ?? {})) {
    if (!prime || prime.vaulted || prime.vault_date) continue;
    const when = at(prime.est_vault_date);
    if (!Number.isFinite(when) || when <= now || when > now + VAULT_HORIZON) continue;
    byDate.set(when, [...(byDate.get(when) ?? []), slug]);
  }
  return [...byDate].map(([when, slugs]) => {
    const names = joinNames(slugs.map(slug => primeName(market, slug)).sort());
    return {
      id: `vault-est-${when}`, kind: 'vault', from: when, to: null, estimate: true, source: 'estimate',
      title: `${names} may be vaulted`, short: `Vaulting (est.) · ${names}`,
      detail: 'Estimated from release dates; DE has not announced it.',
      affects: heldParts(market, slugs, owned),
    } satisfies CalendarEvent;
  });
}

function deals(market: Market, now: number): CalendarEvent[] {
  return (market.de?.deals ?? []).flatMap(deal => {
    const to = at(deal.expiry);
    if (!Number.isFinite(to) || to < now) return [];
    const price = deal.discount && deal.sale_price ? ` (${deal.discount}% off, ${deal.sale_price}p)` : '';
    return [{
      id: `deal-${to}-${deal.item}`, kind: 'deal' as const, from: to - DAY, to, estimate: false, source: 'worldstate' as const,
      title: `Darvo: ${deal.item}${price}`, short: `Darvo · ${deal.item}`,
      detail: 'A store item for real-money Platinum. It does not touch tradeable holdings.',
    }];
  });
}

function gameEvents(market: Market, now: number): CalendarEvent[] {
  const entries = [...Object.values(market.event_rewards?.goals ?? {}), ...Object.values(market.event_rewards?.events ?? {})];
  return entries.flatMap(entry => {
    const from = at(entry.starts_at);
    const to = at(entry.ends_at);
    // DE publishes some goals with no name; an unnamed row tells the reader nothing.
    if (!entry.title?.trim() || !Number.isFinite(from) || !Number.isFinite(to) || to < now - PAST) return [];
    const rewards = [...new Set(entry.groups.flatMap(group => group.rewards.map(reward => reward.name)))];
    return [{
      id: `event-${entry.id}`, kind: 'event' as const, from, to, estimate: false, source: 'worldstate' as const,
      title: entry.title, short: entry.title,
      detail: rewards.length ? `In-game event. Rewards include ${joinNames(rewards.slice(0, 3))}${rewards.length > 3 ? ' and more' : ''}.` : 'In-game event.',
    }];
  });
}

function notices(feed: AnnouncementFeed | null | undefined, now: number): CalendarEvent[] {
  return (feed?.messages ?? []).flatMap(message => {
    const from = at(message.starts_at);
    const to = at(message.ends_at);
    if (!Number.isFinite(from) || !Number.isFinite(to) || to < now - PAST) return [];
    return [{
      id: `notice-${message.id}`, kind: 'notice' as const, from, to, estimate: false, source: 'notice' as const,
      title: message.title, short: message.title, detail: message.body, sourceUrl: message.source_url,
    }];
  });
}

export function calendarEvents(
  market: Market | null | undefined,
  feed: AnnouncementFeed | null | undefined,
  now: number,
  owned?: Map<string, OwnedRecord> | null,
): CalendarEvent[] {
  const events = [
    ...(market ? [...gameEvents(market, now), ...baro(market, now), ...resurgence(market, now, owned), ...vaulting(market, now, owned), ...deals(market, now)] : []),
    ...notices(feed, now),
  ];
  return events.sort((a, b) => a.from - b.from || a.id.localeCompare(b.id));
}

/** The moment an event matters most from `now`: its start, or its end once under way. */
export function eventKey(event: CalendarEvent, now: number): number {
  return event.from <= now && event.to !== null ? event.to : event.from;
}

export function eventEnd(event: CalendarEvent): number {
  return event.to ?? event.from;
}
