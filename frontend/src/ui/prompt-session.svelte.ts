import type { SettingsStore } from '../contracts/state-store';
import { promptDue, readPromptState, withAsk, withDone, withLaunch, type PromptPolicy, type PromptState } from './prompts';

/** One app launch's view of the optional prompts: at most one is asked, and
 *  showing it is what counts as an ask - ignoring it is the same as Not now. */
export class PromptSession {
  /** The prompt this launch shows, once one has won. */
  current = $state<string | null>(null);
  hidden = $state(false);
  #state: PromptState;
  #launched = false;
  #claimed = new Set<string>();
  #notes: 'unknown' | 'pending' | 'settled' = 'unknown';
  #updated = false;
  #passed = new Set<string>();

  /** `order` is highest priority first. */
  constructor(private store: SettingsStore, private order: readonly PromptPolicy[], private now: () => number = Date.now) {
    this.#state = readPromptState(store);
  }

  /** Counts this launch once the user has an inventory: delays and launch
   *  minimums measure use, not installs that never scanned. */
  launch(): void {
    if (this.#launched) return;
    this.#launched = true;
    this.#save(withLaunch(this.#state, this.now()));
    this.#resolve();
  }

  /** A prompt whose own conditions hold asks to be shown. It wins when it is
   *  due and every higher-priority prompt that is due has passed. */
  claim(id: string): void { this.#claimed.add(id); this.#resolve(); }

  /** A prompt that does not apply this launch, so lower ones need not wait. */
  pass(id: string): void { this.#passed.add(id); this.#resolve(); }

  /** Release notes go first: nothing is asked until this launch's automatic
   *  notes are known, and a launch whose notes were dismissed is an update. */
  notes(state: 'pending' | 'none' | 'dismissed'): void {
    if (state === 'pending' && this.#notes === 'settled') return;
    this.#notes = state === 'pending' ? 'pending' : 'settled';
    if (state === 'dismissed') this.#updated = true;
    this.#resolve();
  }

  decline(): void { this.hidden = true; }

  /** Accepted: never asked again, whatever the policy allows. */
  finish(id: string): void { this.#save(withDone(this.#state, id)); }

  #resolve(): void {
    if (this.current !== null || !this.#launched || this.#notes !== 'settled') return;
    const now = this.now();
    for (const policy of this.order) {
      if (this.#passed.has(policy.id) || !promptDue(this.#state, policy, now, this.#updated)) continue;
      if (!this.#claimed.has(policy.id)) return;
      this.current = policy.id;
      this.#save(withAsk(this.#state, policy.id, now));
      return;
    }
  }

  #save(next: PromptState): void {
    this.#state = next;
    // A failed write only means the next launch may ask sooner than planned.
    void this.store.setSetting('prompts', JSON.stringify(next)).catch(() => {});
  }
}
