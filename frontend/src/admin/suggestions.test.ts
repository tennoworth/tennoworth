import { describe, expect, it } from 'vitest';
import type { Market } from '../contracts/data';
import { parseAnnouncements } from '../domain/announcements';
import { calendarSuggestions } from './suggestions';

const NOW = Date.parse('2026-10-12T21:37:42Z');

function market(extra: Partial<Market>): Market {
  return { updated_at: '2026-10-12T20:00:00Z', platform: 'pc', item_count: 0, catalog_count: 0, catalog: {}, items: {}, ...extra };
}
const baro = { activation: '2026-10-16T13:00:00Z', expiry: '2026-10-18T13:00:00Z', location: 'Pluto Relay' };
const rotation = { from: '2026-10-01T18:00:00.000Z', to: '2026-10-15T18:00:00.000Z', frames: ['protea_prime_set', 'ivara_prime_set'] };

describe('calendar suggestions', () => {
  it('every suggestion is a publishable announcement', () => {
    const found = calendarSuggestions(market({
      baro,
      calendar: { resurgence_current: { ...rotation, from: '2026-10-11T18:00:00.000Z' }, primes: { ivara_prime_set: { name: 'Ivara Prime', vaulted: true } } },
    }), NOW);
    expect(found.map(s => s.key)).toEqual(['baro-2026-10-16T13:00:00Z', 'resurgence-start-2026-10-11T18:00:00Z', 'resurgence-end-2026-10-15T18:00:00Z']);
    for (const suggestion of found) {
      const feed = { version: 1, updated_at: '2026-10-12T21:38:00Z', messages: [{ id: 'suggested', ...suggestion.content }] };
      expect(parseAnnouncements(feed), suggestion.key).not.toBeNull();
    }
    expect(found[0].content.starts_at).toBe('2026-10-12T21:37:00Z');
    expect(found[0].content.ends_at).toBe('2026-10-18T13:00:00Z');
    expect(found[0].content.body).toContain('Friday 16 October at 13:00 UTC');
    // A name the calendar lacks still reads as a name, not a slug.
    expect(found[2].content.title).toBe('Protea Prime and Ivara Prime relics leave Prime Resurgence Thursday 15 October');
  });

  it('stays quiet when nothing is close, over or unknown', () => {
    expect(calendarSuggestions(market({}), NOW)).toEqual([]);
    expect(calendarSuggestions(market({ baro: { ...baro, activation: '2026-11-06T13:00:00Z', expiry: '2026-11-08T13:00:00Z' } }), NOW)).toEqual([]);
    expect(calendarSuggestions(market({ baro: { ...baro, activation: '2026-10-02T13:00:00Z', expiry: '2026-10-04T13:00:00Z' } }), NOW)).toEqual([]);
    expect(calendarSuggestions(market({ baro: { ...baro, activation: 'soon' } }), NOW)).toEqual([]);
    // Started long ago and ends beyond the window: nothing new to say.
    expect(calendarSuggestions(market({ calendar: { resurgence_current: { ...rotation, to: '2026-10-29T18:00:00.000Z' } } }), NOW)).toEqual([]);
    expect(calendarSuggestions(market({ calendar: { resurgence_current: { ...rotation, frames: [] } } }), NOW)).toEqual([]);
  });

  it('describes a visit already under way without a past arrival', () => {
    const [here] = calendarSuggestions(market({ baro: { ...baro, activation: '2026-10-11T13:00:00Z', expiry: '2026-10-13T13:00:00Z' } }), NOW);
    expect(here.content.title).toBe("Baro Ki'Teer is here until Tuesday 13 October");
    expect(here.content.body).not.toContain('reaches');
  });
});
