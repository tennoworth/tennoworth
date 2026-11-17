// Which Settings sections are collapsed. A long page is easier to return to
// when the sections someone never changes stay folded, so the choice persists
// (`settings-collapsed`). Everything starts open: a first visit shows it all.
import type { SettingsStore } from '../../contracts/state-store';

export const SETTINGS_SECTIONS = ['scan', 'selling', 'overlay', 'notifications', 'account', 'appearance', 'updates', 'privacy'] as const;
export type SettingsSectionId = typeof SETTINGS_SECTIONS[number];

/** A deep link may name a group inside a section. */
export type SettingsTarget = SettingsSectionId | 'price-sharing';

export function sectionOf(target: SettingsTarget): SettingsSectionId {
  return target === 'price-sharing' ? 'privacy' : target;
}

export function parseCollapsed(raw: string | null): SettingsSectionId[] {
  if (!raw) return [];
  try {
    const value: unknown = JSON.parse(raw);
    return Array.isArray(value) ? SETTINGS_SECTIONS.filter((id) => value.includes(id)) : [];
  } catch {
    return [];
  }
}

export class SettingsSections {
  collapsed = $state<SettingsSectionId[]>([]);

  constructor(private store?: SettingsStore) {
    this.collapsed = parseCollapsed(store?.getSetting('settings-collapsed') ?? null);
  }

  isOpen(id: SettingsSectionId): boolean {
    return !this.collapsed.includes(id);
  }

  toggle(id: SettingsSectionId): void {
    this.#save(this.isOpen(id) ? [...this.collapsed, id] : this.collapsed.filter((c) => c !== id));
  }

  /** Opens the section holding a link's target, so a jump never lands on a fold. */
  reveal(target: SettingsTarget): void {
    const id = sectionOf(target);
    if (!this.isOpen(id)) this.#save(this.collapsed.filter((c) => c !== id));
  }

  #save(next: SettingsSectionId[]): void {
    this.collapsed = SETTINGS_SECTIONS.filter((id) => next.includes(id));
    void this.store?.setSetting('settings-collapsed', JSON.stringify(this.collapsed));
  }
}
