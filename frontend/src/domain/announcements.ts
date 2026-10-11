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

function text(value: unknown, max: number): value is string {
  return typeof value === 'string' && value.trim().length > 0 && Array.from(value).length <= max && !/[\x00-\x08\x0b\x0c\x0e-\x1f\x7f-\x9f]/.test(value);
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
  return value as AnnouncementFeed;
}
