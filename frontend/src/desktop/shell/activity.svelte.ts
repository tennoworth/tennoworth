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
  /** Everything still running, oldest first: finishing one shows the newest
   *  of the rest, so a long scan is not hidden by a short send that ends. */
  #running = new Map<number, Activity>();

  constructor(now: () => number = Date.now) {
    this.#now = now;
  }

  begin(label: string): number {
    const id = ++this.#sequence;
    const activity: Activity = { id, label, tone: 'busy', at: this.#now() };
    this.#running.set(id, activity);
    this.current = activity;
    return id;
  }

  /** Updates the label of activity `id` while it is still running. */
  progress(id: number, label: string): void {
    const activity = this.#running.get(id);
    if (!activity) return;
    const next = { ...activity, label };
    this.#running.set(id, next);
    if (this.current?.id === id) this.current = next;
  }

  /** Settles activity `id`. While other work is still running the newest of
   *  it stays in the cell; the result shows once nothing else is running. */
  finish(id: number, label: string, tone: Exclude<ActivityTone, 'busy'> = 'done'): void {
    this.#running.delete(id);
    const stillRunning = [...this.#running.values()].at(-1);
    this.current = stillRunning ?? { id, label, tone, at: this.#now() };
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
