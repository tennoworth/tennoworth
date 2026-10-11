import { readFileSync } from 'node:fs';
import { parseAnnouncements } from '../frontend/src/domain/announcements';
import limits from '../tests/fixtures/announcements/limits.json';

const raw = readFileSync(new URL('../frontend/public/announcements.json', import.meta.url), 'utf8');
if (Buffer.byteLength(raw) > limits.max_bytes || !parseAnnouncements(JSON.parse(raw))) {
  throw new Error('Invalid announcements.json: check IDs, UTC dates, expiry windows, text limits and source links.');
}
console.log('Announcement feed is valid.');
