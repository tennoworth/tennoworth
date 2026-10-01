import { describe, expect, it } from 'vitest';
import contract from '../../../tests/fixtures/notifications/contracts.json';
import vaultCases from '../../../tests/fixtures/notifications/vault.json';
import { NOTIFICATIONS_EVENT, MARKET_REFRESHED_EVENT } from '../contracts/events';
import { NOTIFICATION_CATEGORIES } from '../contracts/desktop';
import { buildCalendar, vaultAffects } from '../domain/calendar-feed';
import type { Market, OwnedRecord, VaultRotation } from '../contracts/data';
import eventCase from '../../../tests/fixtures/notifications/events.json';

describe('notification contracts shared with Rust', () => {
  it('uses the same event names and categories', () => {
    expect(NOTIFICATIONS_EVENT).toBe(contract.event);
    expect(MARKET_REFRESHED_EVENT).toBe(contract.market_event);
    expect([...NOTIFICATION_CATEGORIES]).toEqual(contract.categories);
  });
  for (const c of vaultCases) it(c.name, () => {
    const held = new Map(c.held.map(slug => [slug, { slug, count: 1 } as OwnedRecord]));
    expect(vaultAffects(c.rotation as VaultRotation, c.market as unknown as Market, held).sort()).toEqual(c.expected);
  });
});

// The native reminder decides which events to notify about from this fixture
// (reminders::event_relevance_matches_the_frontend_fixture); the calendar must
// mark the same events as touching what the player holds. A partial reward list
// with a hit counts; an unknown one and an unrelated one do not.
it('calendar relevance agrees with the native event reminder', () => {
  const held = new Map(eventCase.held.map((slug) => [slug, { slug, count: 1 } as OwnedRecord]));
  const relevant = buildCalendar(eventCase.market as unknown as Market, held, Date.parse(eventCase.now))
    .filter((item) => item.affects.length > 0 && (item.affectsKnown || item.reach === 'partial-hits'))
    .map((item) => item.title);
  expect(relevant).toEqual(eventCase.expected);
});
