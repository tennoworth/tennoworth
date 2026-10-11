import { isHistory, type History } from '../domain/history';
import { loadMarket } from './market';
import { parseAnnouncements } from '../domain/announcements';
import limits from '../../../tests/fixtures/announcements/limits.json';

export class HostedTransport {
  loadMarket = loadMarket;
  async loadAnnouncements() {
    const response = await fetch('/announcements.json', { cache: 'no-cache', signal: AbortSignal.timeout(10_000) });
    if (!response.ok || !response.body) throw new Error('Announcements unavailable');
    const reader = response.body.getReader();
    const chunks: Uint8Array[] = [];
    let length = 0;
    try {
      for (;;) {
        const { done, value } = await reader.read();
        if (done) break;
        length += value.length;
        if (length > limits.max_bytes) throw new Error('Announcement feed too large');
        chunks.push(value);
      }
    } finally { await reader.cancel(); }
    const bytes = new Uint8Array(length);
    let offset = 0;
    for (const chunk of chunks) { bytes.set(chunk, offset); offset += chunk.length; }
    return parseAnnouncements(JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(bytes)));
  }
  async loadHistory(): Promise<History | null> {
    try {
      const r = await fetch('/history.json', { cache: 'no-cache' });
      if (!r.ok) return null;
      const h = (await r.json()) as History;
      return isHistory(h) ? h : null;
    } catch {
      return null;
    }
  }
}
