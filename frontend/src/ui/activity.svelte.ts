// What the app is doing right now, and what it last finished, in one place:
// the status strip's activity cell. Scans, listing sends and live price checks
// each used to report only on their own button, which vanished with the menu
// or dialog that held it. The last result stays until something replaces it.

export type ActivityTone = 'busy' | 'done' | 'bad';

export interface Activity {
  id: number;
  label: string;
  tone: ActivityTone;
  at: number;
}

export class ActivityLog {
  current = $state<Activity | null>(null);
  #sequence = 0;
  #now: () => number;

  constructor(now: () => number = Date.now) {
    this.#now = now;
  }

  begin(label: string): number {
    const id = ++this.#sequence;
    this.current = { id, label, tone: 'busy', at: this.#now() };
    return id;
  }

  /** Updates the label of activity `id` while it is still the one shown. */
  progress(id: number, label: string): void {
    if (this.current?.id === id && this.current.tone === 'busy') this.current = { ...this.current, label };
  }

  /** Settles the activity `id`. A newer activity that started meanwhile keeps
   *  the cell: an older result must not overwrite work still in progress. */
  finish(id: number, label: string, tone: Exclude<ActivityTone, 'busy'> = 'done'): void {
    if (this.current && this.current.id !== id && this.current.tone === 'busy') return;
    this.current = { id, label, tone, at: this.#now() };
  }

  async track<T>(label: string, work: () => Promise<T>, done: (result: T) => string, failed: (error: unknown) => string): Promise<T> {
    const id = this.begin(label);
    try {
      const result = await work();
      this.finish(id, done(result));
      return result;
    } catch (error) {
      this.finish(id, failed(error), 'bad');
      throw error;
    }
  }
}
