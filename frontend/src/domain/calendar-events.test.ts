import { describe, expect, it } from 'vitest';
import type { Market, OwnedRecord } from '../contracts/data';
import { calendarEvents, eventKey } from './calendar-events';

const NOW = Date.parse('2026-10-13T09:00:00Z');

function market(extra: Partial<Market> = {}): Market {
  return { updated_at: '2026-10-12T23:02:00Z', platform: 'pc', item_count: 0, catalog_count: 0, catalog: {}, items: {}, ...extra };
}
function owned(...slugs: string[]): Map<string, OwnedRecord> {
  return new Map(slugs.map(slug => [slug, { count: 1, name: slug, type: '', slug, subtype: null, kept_lvl: null, leveled: 0 }]));
}
const rotation = { from: '2026-10-01T18:00:00.000Z', to: '2026-10-29T18:00:00.000Z', frames: ['protea_prime_set', 'ivara_prime_set'] };

describe('calendarEvents', () => {
  it('marks Baro projections as estimates and keeps the confirmed visit', () => {
    const events = calendarEvents(market({ baro: { activation: '2026-10-16T13:00:00Z', expiry: '2026-10-18T13:00:00Z', location: 'Pluto Relay' } }), null, NOW);
    expect(events.map(e => [e.short, e.estimate, new Date(e.from).toISOString()])).toEqual([
      ['Baro · Pluto Relay', false, '2026-10-16T13:00:00.000Z'],
      ['Baro (est.)', true, '2026-10-30T13:00:00.000Z'],
      ['Baro (est.)', true, '2026-11-13T13:00:00.000Z'],
    ]);
  });

  it('lists a rotation once even when it is both current and in the history', () => {
    const events = calendarEvents(market({ calendar: { resurgence: [rotation], resurgence_current: rotation } }), null, NOW);
    expect(events).toHaveLength(1);
    expect(events[0].title).toBe('Prime Resurgence: Protea Prime and Ivara Prime');
    // Under way, so it is filed under the date it ends.
    expect(new Date(eventKey(events[0], NOW)).toISOString()).toBe('2026-10-29T18:00:00.000Z');
  });

  it('knows which held parts a rotation touches, and says nothing without an inventory', () => {
    const data = market({
      calendar: { resurgence_current: rotation },
      set_to_parts: { ivara_prime_set: { name: 'Ivara Prime Set', parts: [{ slug: 'ivara_prime_chassis_blueprint', component_name: 'Chassis' }] } },
    });
    expect(calendarEvents(data, null, NOW, owned('ivara_prime_chassis_blueprint', 'lavos_prime_set'))[0].affects).toEqual(['ivara_prime_chassis_blueprint']);
    expect(calendarEvents(data, null, NOW)[0].affects).toBeUndefined();
  });

  it('groups estimated vaulting by date and skips confirmed, vaulted, past and distant ones', () => {
    const events = calendarEvents(market({ calendar: { primes: {
      lavos_prime_set: { name: 'Lavos Prime', vaulted: false, est_vault_date: '2026-11-12' },
      cedo_prime_set: { name: 'Cedo Prime', vaulted: false, est_vault_date: '2026-11-12' },
      braton_prime_set: { name: 'Braton Prime', vaulted: false, est_vault_date: '2015-04-13' },
      yareli_prime_set: { name: 'Yareli Prime', vaulted: false, est_vault_date: '2027-02-21' },
      acceltra_prime_set: { name: 'Acceltra Prime', vaulted: true, est_vault_date: '2026-11-01', vault_date: '2026-11-01' },
    } } }), null, NOW);
    expect(events.map(e => [e.title, e.estimate, e.to])).toEqual([['Cedo Prime and Lavos Prime may be vaulted', true, null]]);
  });

  it('includes curated notices with their source, and drops long-finished events', () => {
    const feed = { version: 1, updated_at: '2026-10-11T19:00:00Z', messages: [
      { id: 'bronco', title: 'Bronco Prime Barrel: 45 → 25 Ducats', body: 'Exchange spare barrels.', source_url: 'https://forums.warframe.com/', starts_at: '2026-10-11T00:00:00Z', ends_at: '2026-10-15T15:00:00Z' },
      { id: 'old', title: 'Old', body: 'Old.', source_url: 'https://forums.warframe.com/', starts_at: '2026-07-01T00:00:00Z', ends_at: '2026-07-02T00:00:00Z' },
    ] };
    const events = calendarEvents(null, feed, NOW);
    expect(events.map(e => [e.id, e.kind, e.sourceUrl])).toEqual([['notice-bronco', 'notice', 'https://forums.warframe.com/']]);
  });

  it('survives missing and malformed surfaces', () => {
    expect(calendarEvents(null, null, NOW)).toEqual([]);
    expect(calendarEvents(market({ baro: { activation: 'soon', expiry: '', location: '' }, calendar: { resurgence_current: { ...rotation, frames: [] } }, de: { deals: [{ item: 'X', expiry: 'never' }] } }), null, NOW)).toEqual([]);
  });

  it('lists named DE events with a few of their rewards and skips unnamed ones', () => {
    const goal = (id: string, title: string, rewards: string[]) => ({ id, source: 'goal' as const, title, starts_at: '2026-10-07T16:00:00Z', ends_at: '2026-10-21T16:00:00Z', completeness: 'complete' as const,
      groups: [{ kind: 'final' as const, rewards: rewards.map(name => ({ unique: `/Lotus/${name}`, name, quantity: 1 })) }] });
    const events = calendarEvents(market({ event_rewards: { goals: { a: goal('a', 'Heat Fissure', ['Kuva', 'Ayatan Sculpture', 'Riven Mod', 'Forma']), b: goal('b', '', ['Kuva']) } } }), null, NOW);
    expect(events.map(e => [e.kind, e.title, e.detail])).toEqual([['event', 'Heat Fissure', 'In-game event. Rewards include Kuva, Ayatan Sculpture and Riven Mod and more.']]);
  });

  it('turns a live Darvo deal into a one-day event', () => {
    const [deal] = calendarEvents(market({ de: { deals: [{ item: 'Tenno Sword Shield', expiry: '2026-10-13T15:00:00Z', discount: 90, sale_price: 15 }] } }), null, NOW);
    expect(deal.title).toBe('Darvo: Tenno Sword Shield (90% off, 15p)');
    expect(deal.to! - deal.from).toBe(86_400_000);
  });
});
