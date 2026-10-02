import type { LiveTop, LiveTopQuery } from '../../contracts/desktop';
import type { DesktopServices } from '../../contracts/services';
import { LIVE_TOP_PROGRESS_EVENT } from '../../contracts/events';
import { humanError } from '../../contracts/errors';

type Services = Pick<DesktopServices, 'desktopLiveTopPrices' | 'listenForTauriEvent'>;

function key(query: LiveTopQuery): string {
  return `${query.slug}|${query.rank ?? 0}|${query.subtype ?? ''}`;
}

export class LiveTopController {
  phase = $state<'idle' | 'running' | 'done' | 'error'>('idle');
  progress = $state({ done: 0, total: 0 });
  error = $state<string | null>(null);
  quotes = $state<Map<string, LiveTop>>(new Map());
  private unlisten?: () => void;
  private disposed = false;

  constructor(private services: Services) {}

  get(query: LiveTopQuery): LiveTop | undefined {
    return this.quotes.get(key(query));
  }

  async check(queries: LiveTopQuery[]): Promise<LiveTop[] | null> {
    if (this.disposed || this.phase === 'running' || queries.length === 0) return null;
    this.phase = 'running';
    this.error = null;
    this.progress = { done: 0, total: queries.length };
    try {
      this.unlisten ??= this.services.listenForTauriEvent<{ done: number; total: number }>(LIVE_TOP_PROGRESS_EVENT, progress => {
        // The event channel is shared by every live-price surface.
        if (!this.disposed && this.phase === 'running') this.progress = progress;
      });
      const quotes = await this.services.desktopLiveTopPrices(queries);
      if (this.disposed) return null;
      const next = new Map(this.quotes);
      for (const quote of quotes) next.set(key(quote), quote);
      this.quotes = next;
      this.phase = 'done';
      return quotes;
    } catch (error) {
      if (!this.disposed) {
        this.phase = 'error';
        this.error = humanError(error);
      }
      return null;
    }
  }

  dispose(): void {
    this.disposed = true;
    this.unlisten?.();
    this.unlisten = undefined;
  }
}
