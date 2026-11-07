// Relics the player chose to crack despite the advice to sell them intact or
// refine them first. The advice stays visible; the row's value and the total
// follow the player's call, and the choice is kept on this device.
import type { SettingsStore } from '../../contracts/state-store';

export class CrackAnyway {
  slugs = $state<Set<string>>(new Set());

  constructor(private store: SettingsStore) {
    try {
      const saved: unknown = JSON.parse(store.getSetting('relic-crack-anyway') ?? '[]');
      if (Array.isArray(saved)) this.slugs = new Set(saved.filter((slug): slug is string => typeof slug === 'string'));
    } catch { /* A malformed value means no overrides. */ }
  }

  has(slug: string): boolean { return this.slugs.has(slug); }

  set(slug: string, crack: boolean): void {
    const next = new Set(this.slugs);
    if (crack) next.add(slug); else next.delete(slug);
    this.slugs = next;
    void this.store.setSetting('relic-crack-anyway', JSON.stringify([...next].sort())).catch(() => {});
  }
}
