import { afterEach, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen, within } from '@testing-library/svelte';
import type { Market } from '../../contracts/data';
import type { SettingKey, SettingsStore } from '../../contracts/state-store';
import CalendarView from './CalendarView.svelte';

afterEach(cleanup);

const NOW = Date.parse('2026-10-13T09:00:00Z');
const market = {
  updated_at: '2026-10-12T23:02:00Z', platform: 'pc', item_count: 0, catalog_count: 0, catalog: {}, items: {},
  baro: { activation: '2026-10-16T13:00:00Z', expiry: '2026-10-18T13:00:00Z', location: 'Pluto Relay' },
  calendar: { resurgence_current: { from: '2026-10-01T18:00:00.000Z', to: '2026-10-29T18:00:00.000Z', frames: ['protea_prime_set'] } },
  de: { deals: [{ item: 'Tenno Sword Shield', expiry: '2026-10-13T15:00:00Z' }] },
} as Market;
const feed = { version: 1, updated_at: '2026-10-11T19:00:00Z', messages: [{ id: 'bronco', title: 'Bronco Prime Barrel: 45 → 25 Ducats', body: 'Exchange spare barrels.', source_url: 'https://forums.warframe.com/', starts_at: '2026-10-11T00:00:00Z', ends_at: '2026-10-15T15:00:00Z' }] };

function store(initial: Partial<Record<SettingKey, string>> = {}): SettingsStore & { saved: Record<string, string> } {
  const saved: Record<string, string> = { ...initial };
  return {
    saved, mode: 'local', hydrate: async () => {},
    getSetting: (key: SettingKey) => saved[key] ?? null,
    setSetting: vi.fn(async (key: SettingKey, value: string) => { saved[key] = value; }),
  };
}

describe('CalendarView', () => {
  it('opens on the board and counts down to the soonest confirmed moment, a deadline included', async () => {
    render(CalendarView, { props: { market, now: NOW, surface: 'hosted', loadAnnouncements: async () => feed } });
    const next = screen.getByRole('region', { name: 'Next up' });
    expect(await within(next).findByText('Bronco Prime Barrel: 45 → 25 Ducats')).toBeTruthy();
    expect(within(next).getByText('Ends in')).toBeTruthy();
    expect(within(next).getByText('2d 06h')).toBeTruthy();
    // Darvo starts hidden; estimates are labelled.
    expect(screen.queryByText(/Tenno Sword Shield/)).toBeNull();
    expect(screen.getAllByText('est.').length).toBeGreaterThan(0);
  });

  it('remembers the view and the hidden kinds', async () => {
    const settings = store();
    render(CalendarView, { props: { market, now: NOW, surface: 'hosted', store: settings } });
    await fireEvent.click(screen.getByRole('button', { name: 'Lanes' }));
    await fireEvent.click(screen.getByRole('checkbox', { name: "Baro Ki'Teer" }));
    expect(JSON.parse(settings.saved.calendar)).toEqual({ view: 'lanes', hidden: ['deal', 'baro'] });
    cleanup();
    render(CalendarView, { props: { market, now: NOW, surface: 'hosted', store: settings } });
    expect(screen.getByRole('button', { name: 'Lanes' }).getAttribute('aria-pressed')).toBe('true');
    expect(screen.queryByRole('button', { name: /Baro Ki'Teer at Pluto Relay/ })).toBeNull();
  });

  it('ignores a corrupt saved choice', () => {
    render(CalendarView, { props: { market, now: NOW, surface: 'hosted', store: store({ calendar: '{not json' }) } });
    expect(screen.getByRole('button', { name: 'Board' }).getAttribute('aria-pressed')).toBe('true');
  });

  it('keeps focus on a chosen day and lists what happens on it', async () => {
    render(CalendarView, { props: { market, now: NOW, surface: 'hosted', store: store({ calendar: '{"view":"month","hidden":["deal"]}' }) } });
    const friday = screen.getByRole('button', { name: /^Friday 16 October, 2 events/ });
    friday.focus();
    await fireEvent.click(friday);
    expect(document.activeElement).toBe(friday);
    expect(friday.getAttribute('aria-pressed')).toBe('true');
    const day = screen.getByRole('region', { name: 'Selected day' });
    expect(within(day).getByText("Baro Ki'Teer at Pluto Relay")).toBeTruthy();
    expect(within(day).getAllByText('DE worldState')).toHaveLength(2);
  });

  it('still shows the game calendar when the notice feed fails', async () => {
    render(CalendarView, { props: { market, now: NOW, surface: 'hosted', loadAnnouncements: async () => { throw new Error('offline'); } } });
    const next = screen.getByRole('region', { name: 'Next up' });
    expect(await within(next).findByText("Baro Ki'Teer at Pluto Relay")).toBeTruthy();
  });

  it('says on desktop how many held items an event touches', async () => {
    const owned = new Map([['protea_prime_set|', { slug: 'protea_prime_set', count: 1, name: '', type: '', subtype: null, kept_lvl: null, leveled: 0 }]]);
    render(CalendarView, { props: { market, owned, now: NOW, surface: 'desktop' } });
    expect(screen.getByText('You hold 1 affected item')).toBeTruthy();
  });
});
