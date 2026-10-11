import { expect, it } from 'vitest';
import { activeAnnouncements, parseAnnouncements } from './announcements';
import cases from '../../../tests/fixtures/announcements/cases.json';
import published from '../../public/announcements.json';

it.each(cases)('announcement contract: $name', ({ feed, valid, now, active_ids }) => {
  const parsed = parseAnnouncements(feed);
  expect(parsed !== null).toBe(valid);
  expect(activeAnnouncements(parsed, Date.parse(now)).map(message => message.id)).toEqual(active_ids);
});

it('the published feed satisfies the reader contract', () => {
  expect(parseAnnouncements(published)).not.toBeNull();
});
