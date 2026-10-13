// Announcement drafts proposed from dates the scraper already publishes.
//
// Suggestions only fill the editor: each still goes through save, review and
// publish, and its wording is a starting point to check against the source.

import type { Market, ResurgenceRotation } from '../contracts/data';
import type { Content } from '../contracts/generated/publisher';
import { joinNames, primeName } from '../domain/calendar-events';

export interface Suggestion {
  /** Stable for one event, so a dismissal outlives a reload but not the next visit. */
  key: string;
  headline: string;
  detail: string;
  content: Content;
}

const DAY = 86_400_000;
const BARO_AHEAD = 14 * DAY;
const RESURGENCE_ENDING = 10 * DAY;
const RESURGENCE_STARTED = 3 * DAY;
const BARO_SOURCE = 'https://wiki.warframe.com/w/Baro_Ki%27Teer';
const RESURGENCE_SOURCE = 'https://wiki.warframe.com/w/Prime_Resurgence';

function stamp(ms: number): string {
  return new Date(ms).toISOString().replace(/\.\d{3}Z$/, 'Z');
}
function day(ms: number): string {
  return new Date(ms).toLocaleDateString('en-GB', { weekday: 'long', day: 'numeric', month: 'long', timeZone: 'UTC' });
}
function time(ms: number): string {
  return new Date(ms).toLocaleTimeString('en-GB', { hour: '2-digit', minute: '2-digit', timeZone: 'UTC' });
}
function when(ms: number): string {
  return `${day(ms)} at ${time(ms)} UTC`;
}

function names(market: Market, rotation: ResurgenceRotation): string {
  return joinNames(rotation.frames.map(slug => primeName(market, slug)));
}

function baro(market: Market, now: number): Suggestion[] {
  const visit = market.baro;
  const arrives = Date.parse(visit?.activation ?? '');
  const leaves = Date.parse(visit?.expiry ?? '');
  if (!visit || !Number.isFinite(arrives) || !Number.isFinite(leaves) || leaves <= now || arrives - now > BARO_AHEAD) return [];
  const where = visit.location || 'a relay';
  const here = arrives <= now;
  return [{
    key: `baro-${stamp(arrives)}`,
    headline: here ? `Baro Ki'Teer is at ${where} until ${day(leaves)}` : `Baro Ki'Teer arrives ${day(arrives)}, ${time(arrives)} UTC`,
    detail: `${where}, ${Math.round((leaves - arrives) / 3_600_000)} h. The draft ends when he leaves.`,
    content: {
      title: here ? `Baro Ki'Teer is here until ${day(leaves)}` : `Baro Ki'Teer arrives ${day(arrives)}`,
      body: `Baro Ki'Teer ${here ? 'is at' : 'reaches'} ${where}${here ? '' : ` on ${when(arrives)}`} and leaves on ${when(leaves)}.\n`
        + 'Items he sells often fall in price while he is here, as fresh copies reach the market. '
        + 'If you have spare copies of his stock to sell, listing them before the new supply arrives usually gets the better price.',
      source_url: BARO_SOURCE,
      starts_at: stamp(Math.floor(now / 60_000) * 60_000),
      ends_at: stamp(leaves),
    },
  }];
}

function resurgence(market: Market, now: number): Suggestion[] {
  const rotation = market.calendar?.resurgence_current;
  const from = Date.parse(rotation?.from ?? '');
  const to = Date.parse(rotation?.to ?? '');
  if (!rotation?.frames.length || !Number.isFinite(from) || !Number.isFinite(to) || to <= now) return [];
  const who = names(market, rotation);
  const start = stamp(Math.floor(now / 60_000) * 60_000);
  const found: Suggestion[] = [];
  if (from <= now && now - from <= RESURGENCE_STARTED) {
    found.push({
      key: `resurgence-start-${stamp(from)}`,
      headline: `${who} relics returned to Prime Resurgence`,
      detail: `Available until ${day(to)}. Fresh supply tends to lower part and set prices.`,
      content: {
        title: `${who} relics are back in Prime Resurgence`,
        body: `Varzia offers ${who} relics until ${when(to)}.\n`
          + 'New supply usually pushes part and set prices down during the rotation. '
          + 'If you hold spare parts you plan to sell anyway, selling early usually beats waiting.',
        source_url: RESURGENCE_SOURCE,
        starts_at: start,
        ends_at: stamp(to),
      },
    });
  }
  if (to - now <= RESURGENCE_ENDING) {
    found.push({
      key: `resurgence-end-${stamp(to)}`,
      headline: `${who} relics leave Prime Resurgence ${day(to)}`,
      detail: `Ends ${time(to)} UTC. Prices tend to recover once the relics stop dropping.`,
      content: {
        title: `${who} relics leave Prime Resurgence ${day(to)}`,
        body: `Prime Resurgence stops offering ${who} relics on ${when(to)}.\n`
          + 'After that, new parts come only from relics already in circulation, and prices usually rise over the following weeks. '
          + 'Hold spare parts rather than undercutting now, unless you need the Platinum.',
        source_url: RESURGENCE_SOURCE,
        starts_at: start,
        ends_at: stamp(to),
      },
    });
  }
  return found;
}

export function calendarSuggestions(market: Market, now: number): Suggestion[] {
  return [...baro(market, now), ...resurgence(market, now)];
}
