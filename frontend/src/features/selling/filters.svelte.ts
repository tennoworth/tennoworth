import type { StateStore } from '../../contracts/state-store';
import { presetFilterValues } from '../../domain/presets';
const VALID_VIEWS: ReadonlySet<View> = new Set(['sell', 'session', 'sets', 'relics', 'rivens', 'baro', 'routines', 'meta', 'orders', 'watches', 'ledger', 'notifications', 'install', 'settings']);
export type View = 'sell' | 'session' | 'sets' | 'relics' | 'rivens' | 'baro' | 'routines' | 'meta' | 'orders' | 'watches' | 'ledger' | 'notifications' | 'install' | 'settings';
/** A malformed or hand-edited value falls back to the presets' own columns. */
export function parseColumnChoice(raw: string | null): Record<string, string[]> {
  if (!raw) return {};
  try {
    const value: unknown = JSON.parse(raw);
    if (!value || typeof value !== 'object' || Array.isArray(value)) return {};
    return Object.fromEntries(Object.entries(value).filter((entry): entry is [string, string[]] =>
      Array.isArray(entry[1]) && entry[1].length > 0 && entry[1].every((key) => typeof key === 'string')));
  } catch {
    return {};
  }
}
export class FilterController {
  constructor(private store: StateStore) {
    this.reserveCopies = (() => {
      const n = parseInt(this.store.getSetting('reserve-copies') ?? '', 10);
      return Number.isFinite(n) && n >= 0 ? n : 0;
    })();
    this.filtersOpen = (() => this.store.getSetting('filters-open') === '1')();
    this.view = (() => {
      const saved = this.store.getSetting('view') as View | null;
      return saved && VALID_VIEWS.has(saved) ? saved : 'sell';
    })();
    this.scoreExplainerDismissed = (() => this.store.getSetting('score-explainer-dismissed') === '1')();
    this.sellOnboardingDismissed = (() => this.store.getSetting('sell-onboarding-dismissed') === '1')();
    this.keepCopiesNudgeDismissed = (() => this.store.getSetting('keep-copies-nudge-dismissed') === '1')();
    this.columnChoice = parseColumnChoice(this.store.getSetting('sell-columns'));
  }
  minPrice = $state(5);
  minOwned = $state(1);
  reserveCopies = $state(
    0
  );
  typeFilter = $state('all');
  hideAtLvl = $state(5);
  activeTags = $state<Set<string>>(new Set());
  filtersOpen = $state(false);
  view = $state<View>(
    'sell'
  );
  scoreExplainerDismissed = $state(false);
  sellOnboardingDismissed = $state(false);
  keepCopiesNudgeDismissed = $state(false);
  activePreset = $state<string | null>('default');
  // Columns the user picked, per preset ('custom' when none is active). A
  // preset without an entry shows its own column set.
  columnChoice = $state<Record<string, string[]>>({});
  get columnKey(): string { return this.activePreset ?? 'custom'; }
  setColumns(columns: string[] | null): void {
    const next = { ...this.columnChoice };
    if (columns) next[this.columnKey] = columns; else delete next[this.columnKey];
    this.columnChoice = next;
    void this.store.setSetting('sell-columns', JSON.stringify(next));
  }
  setReserveCopies(e: number | Event) {
    const raw = typeof e === 'number' ? e : (e.currentTarget as HTMLInputElement).value;
    const n = Math.max(0, parseInt(String(raw), 10) || 0);
    this.reserveCopies = n;
    void this.store.setSetting('reserve-copies', String(n));
  }
  toggleFiltersOpen(e: Event): void {
    const isOpen = (e.currentTarget as HTMLDetailsElement).open;
    this.filtersOpen = isOpen;
    void this.store.setSetting('filters-open', isOpen ? '1' : '0');
  }
  setView(v: View): void {
    this.view = v;
    void this.store.setSetting('view', v);
  }
  dismissScoreExplainer(): void {
    this.scoreExplainerDismissed = true;
    void this.store.setSetting('score-explainer-dismissed', '1');
  }
  dismissSellOnboarding(): void {
    this.sellOnboardingDismissed = true;
    void this.store.setSetting('sell-onboarding-dismissed', '1');
  }
  dismissKeepCopiesNudge(): void {
    this.keepCopiesNudgeDismissed = true;
    void this.store.setSetting('keep-copies-nudge-dismissed', '1');
  }
  applyPreset(name: string): void {
    const values = presetFilterValues(name);
    if (!values) return;
    this.minPrice = values.minPrice;
    this.hideAtLvl = values.hideAtLvl;
    this.typeFilter = values.typeFilter;
    this.activeTags = values.activeTags;
    this.activePreset = name;
  }
}
