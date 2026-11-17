import { describe, it, expect, beforeEach } from 'vitest';
import { LocalStorageStateStore, LOCAL_SETTING_KEYS } from '../../adapters/state-store';
import { SettingsSections, parseCollapsed } from './sections.svelte';

beforeEach(() => localStorage.clear());

describe('SettingsSections', () => {
  it('starts with every section open', () => {
    const sections = new SettingsSections(new LocalStorageStateStore());
    expect(sections.collapsed).toEqual([]);
    expect(sections.isOpen('overlay')).toBe(true);
  });

  it('keeps a fold across visits, in page order', () => {
    const store = new LocalStorageStateStore();
    const first = new SettingsSections(store);
    first.toggle('privacy');
    first.toggle('overlay');
    expect(localStorage.getItem(LOCAL_SETTING_KEYS['settings-collapsed'])).toBe('["overlay","privacy"]');
    const next = new SettingsSections(store);
    expect(next.isOpen('overlay')).toBe(false);
    expect(next.isOpen('privacy')).toBe(false);
    next.toggle('overlay');
    expect(new SettingsSections(store).collapsed).toEqual(['privacy']);
  });

  it('unfolds the section a link points into, including a group inside one', () => {
    const sections = new SettingsSections(new LocalStorageStateStore());
    sections.toggle('privacy');
    sections.toggle('account');
    sections.reveal('price-sharing');
    expect(sections.isOpen('privacy')).toBe(true);
    expect(sections.isOpen('account')).toBe(false);
  });

  it('reads an unreadable or foreign value as all open', () => {
    expect(parseCollapsed(null)).toEqual([]);
    expect(parseCollapsed('not json')).toEqual([]);
    expect(parseCollapsed('{"overlay":true}')).toEqual([]);
    expect(parseCollapsed('["app","overlay",3]')).toEqual(['overlay']);
  });

  it('folds for the visit alone without a store', () => {
    const sections = new SettingsSections();
    sections.toggle('scan');
    expect(sections.isOpen('scan')).toBe(false);
  });
});
