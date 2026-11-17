// The player's price rule and order-health preferences, read once at boot and
// shared by Settings and My orders, so a rule changed in Settings re-judges the
// listings on screen without a restart.
import type { SettingsStore } from '../../contracts/state-store';
import { DEFAULT_PRICE_RULE, parsePriceRule, type PriceRule } from '../../domain/price-rule';
import { parseHealthPrefs, type HealthPrefs } from '../../domain/listing-health';

export function isDefaultRule(rule: PriceRule): boolean {
  return rule.basis === DEFAULT_PRICE_RULE.basis && rule.offset === DEFAULT_PRICE_RULE.offset;
}

export class PricingController {
  rule = $state<PriceRule>(DEFAULT_PRICE_RULE);
  #health = $state<string | null>(null);
  healthPrefs: HealthPrefs = $derived(parseHealthPrefs(this.#health, this.rule));

  constructor(private store: SettingsStore) {
    this.rule = parsePriceRule(store.getSetting('price-rule'));
    this.#health = store.getSetting('listing-health');
  }

  async setRule(next: PriceRule): Promise<void> {
    const previous = this.rule;
    this.rule = next;
    try {
      await this.store.setSetting('price-rule', JSON.stringify(next));
    } catch (error) {
      this.rule = previous;
      throw error;
    }
  }
}
