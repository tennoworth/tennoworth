import type { LiveTop, LiveTopQuery } from '../../contracts/desktop';
import type { DesktopServices } from '../../contracts/services';
import { LIVE_TOP_PROGRESS_EVENT } from '../../contracts/events';
import { humanError } from '../../contracts/errors';

type Services = Pick<DesktopServices, 'desktopLiveTopPrices' | 'listenForTauriEvent'>;

function key(query: LiveTopQuery): string {
  return `${query.slug}|${query.rank ?? 0}|${query.subtype ?? ''}`;
}

// Process-wide, because every controller hears the one progress channel.
let lastRequestId = 0;

export class LiveTopController {
  phase = $state<'idle' | 'running' | 'done' | 'error'>('idle');
  progress = $state({ done: 0, total: 0 });
  error = $state<string | null>(null);
  quotes = $state<Map<string, LiveTop>>(new Map());
  /** When the last successful check finished; null while nothing is retained. */
  checkedAt = $state<number | null>(null);
  private unlisten?: () => void;
  private disposed = false;
  private generation = 0;
  private requestId: number | null = null;

  constructor(private services: Services) {}

  get(query: LiveTopQuery): LiveTop | undefined {
    return this.quotes.get(key(query));
  }

  async check(queries: LiveTopQuery[]): Promise<LiveTop[] | null> {
    if (this.disposed || this.phase === 'running' || queries.length === 0) return null;
    const generation = this.generation;
    const requestId = ++lastRequestId;
    this.requestId = requestId;
    this.phase = 'running';
    this.error = null;
    this.progress = { done: 0, total: queries.length };
    try {
      this.unlisten ??= this.services.listenForTauriEvent<{ request_id: number; done: number; total: number }>(LIVE_TOP_PROGRESS_EVENT, ({ request_id, done, total }) => {
        if (!this.disposed && this.phase === 'running' && request_id === this.requestId) this.progress = { done, total };
      });
      const quotes = await this.services.desktopLiveTopPrices(queries, requestId);
      if (this.disposed || generation !== this.generation) return null;
      const next = new Map(this.quotes);
      for (const quote of quotes) next.set(key(quote), quote);
      this.quotes = next;
      this.checkedAt = Date.now();
      this.phase = 'done';
      return quotes;
    } catch (error) {
      if (!this.disposed && generation === this.generation) {
        this.phase = 'error';
        this.error = humanError(error);
      }
      return null;
    }
  }

  /** Forget every quote and disown any request still in flight. Live prices
   *  depend on the signed-in account (its platform, and which orders are its
   *  own), so a quote from another session must never be shown or adopted. */
  reset(): void {
    this.generation += 1;
    this.requestId = null;
    this.quotes = new Map();
    this.checkedAt = null;
    this.error = null;
    this.progress = { done: 0, total: 0 };
    this.phase = 'idle';
  }

  dispose(): void {
    this.disposed = true;
    this.unlisten?.();
    this.unlisten = undefined;
  }
}
