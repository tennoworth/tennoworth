import { describe, expect, it } from 'vitest';
import contract from '../../../tests/fixtures/notifications/contracts.json';
import vaultCases from '../../../tests/fixtures/notifications/vault.json';
import { NOTIFICATIONS_EVENT, MARKET_REFRESHED_EVENT } from '../contracts/events';
import { NOTIFICATION_CATEGORIES } from '../contracts/desktop';
import { vaultAffects } from '../domain/calendar-feed';
import type { Market, OwnedRecord, VaultRotation } from '../contracts/data';

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

import eventCase from '../../../tests/fixtures/notifications/events.json';
import { affecting, buildCalendar } from '../domain/calendar-feed';
it('calendar relevance preserves partial hits and excludes unknowns', () => {
  const held = new Map(eventCase.held.map(slug => [slug, { slug, count: 1 } as OwnedRecord]));
  expect(affecting(buildCalendar(eventCase.market as unknown as Market, held, Date.parse(eventCase.now))).map(e => e.title)).toEqual(eventCase.expected);
});
