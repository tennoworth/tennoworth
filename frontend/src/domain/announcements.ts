import type { AnnouncementFeed } from '../contracts/generated/desktop';
import limits from '../../../tests/fixtures/announcements/limits.json';

export function activeAnnouncements(feed: AnnouncementFeed | null, now: number) {
  return feed?.messages.filter(message => now >= Date.parse(message.starts_at) && now < Date.parse(message.ends_at)) ?? [];
}

function stamp(value: unknown): value is string {
  if (typeof value !== 'string' || !/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$/.test(value)) return false;
  const parsed = Date.parse(value);
  return Number.isFinite(parsed) && new Date(parsed).toISOString().replace('.000Z', 'Z') === value;
}

// Bidirectional controls can make a title or deadline read differently from its stored text.
function text(value: unknown, max: number): value is string {
  return typeof value === 'string' && value.trim().length > 0 && Array.from(value).length <= max && !/[\x00-\x08\x0b\x0c\x0e-\x1f\x7f-\x9f\u061c\u200e\u200f\u202a-\u202e\u2066-\u2069]/.test(value);
}

export function announcementSource(value: string): string | null {
  try {
    const source = new URL(value);
    return source.protocol === 'https:' && !source.username && !source.password && !source.port && limits.source_hosts.includes(source.hostname) ? source.href : null;
  } catch { return null; }
}

export function parseAnnouncements(value: unknown): AnnouncementFeed | null {
  if (!value || typeof value !== 'object') return null;
  const feed = value as Record<string, unknown>;
  if (new TextEncoder().encode(JSON.stringify(value)).length > limits.max_bytes) return null;
  if (feed.version !== 1 || !stamp(feed.updated_at) || !Array.isArray(feed.messages) || feed.messages.length > limits.max_messages) return null;
  const ids = new Set<string>();
  for (const raw of feed.messages) {
    if (!raw || typeof raw !== 'object') return null;
    const message = raw as Record<string, unknown>;
    if (!text(message.id, limits.max_id) || !/^[a-z0-9_-]+$/.test(message.id) || ids.has(message.id)
      || !text(message.title, limits.max_title) || !text(message.body, limits.max_body)
      || !stamp(message.starts_at) || !stamp(message.ends_at) || typeof message.source_url !== 'string') return null;
    const window = Date.parse(message.ends_at) - Date.parse(message.starts_at);
    if (window <= 0 || window > limits.max_window_seconds * 1000) return null;
    if (!announcementSource(message.source_url)) return null;
    ids.add(message.id);
  }
  if (feed.events !== undefined && feed.events !== null && !validEvents(feed.events)) return null;
  return value as AnnouncementFeed;
}

function validEvents(value: unknown): boolean {
  if (!Array.isArray(value) || value.length > limits.max_events) return false;
  const ids = new Set<string>();
  for (const raw of value) {
    if (!raw || typeof raw !== 'object') return false;
    const event = raw as Record<string, unknown>;
    if (!text(event.id, limits.max_id) || !/^[a-z0-9_-]+$/.test(event.id) || ids.has(event.id)
      || typeof event.kind !== 'string' || !limits.event_kinds.includes(event.kind)
      || !text(event.title, limits.max_title) || !stamp(event.starts_at)
      || typeof event.source_url !== 'string' || !announcementSource(event.source_url)) return false;
    if (event.ends_at !== undefined && event.ends_at !== null) {
      if (!stamp(event.ends_at)) return false;
      const window = Date.parse(event.ends_at) - Date.parse(event.starts_at);
      if (window <= 0 || window > limits.max_event_window_seconds * 1000) return false;
    }
    if (event.note !== undefined && event.note !== null && !text(event.note, limits.max_event_note)) return false;
    ids.add(event.id);
  }
  return true;
}
