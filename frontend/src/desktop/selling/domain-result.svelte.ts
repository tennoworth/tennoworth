import { humanError } from '../../contracts/errors';

export class DomainResult<T> {
  value = $state<T>() as T;
  phase = $state<'idle' | 'loading' | 'done' | 'error'>('idle');
  error = $state<string | null>(null);
  private generation = 0;

  constructor(private empty: () => T) {
    this.value = empty();
  }

  clear() {
    this.generation += 1;
    this.value = this.empty();
    this.phase = 'idle';
    this.error = null;
  }

  start(load: () => Promise<T>): () => void {
    const generation = ++this.generation;
    this.value = this.empty();
    this.phase = 'loading';
    this.error = null;
    void Promise.resolve().then(load).then(value => {
      if (generation !== this.generation) return;
      this.value = value;
      this.phase = 'done';
    }).catch(error => {
      if (generation !== this.generation) return;
      this.error = humanError(error);
      this.phase = 'error';
    });
    return () => { if (generation === this.generation) this.generation += 1; };
  }
}
