import { isHistory, type History } from '../domain/history';
import { loadMarket } from './market';

export class HostedTransport {
  loadMarket = loadMarket;
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
