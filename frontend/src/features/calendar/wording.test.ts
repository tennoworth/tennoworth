import { afterEach, describe, expect, it } from 'vitest';
import { i18n } from '../../i18n/i18n.svelte';
import { eventWords } from './wording';

afterEach(() => i18n.use('en'));

describe('eventWords', () => {
  it('words Baro, with or without a known relay', () => {
    expect(eventWords({ kind: 'baro', where: 'Pluto Relay' })).toMatchObject({ title: 'Baro Ki’Teer at Pluto Relay', short: 'Baro · Pluto Relay' });
    expect(eventWords({ kind: 'baro', where: null }).title).toBe('Baro Ki’Teer at a relay');
    expect(eventWords({ kind: 'baro-estimate' }).short).toBe('Baro (est.)');
  });

  it('lists names the way the English calendar always did', () => {
    expect(eventWords({ kind: 'resurgence', names: ['Protea Prime', 'Ivara Prime'] }).title).toBe('Prime Resurgence: Protea Prime and Ivara Prime');
    expect(eventWords({ kind: 'vault', names: ['Cedo Prime', 'Lavos Prime'] }).title).toBe('Cedo Prime and Lavos Prime may be vaulted');
  });

  it('names three rewards and says when there are more', () => {
    expect(eventWords({ kind: 'event', title: 'Heat Fissure', rewards: ['Kuva', 'Ayatan Sculpture', 'Riven Mod', 'Forma'] }).detail)
      .toBe('In-game event. Rewards include Kuva, Ayatan Sculpture and Riven Mod and more.');
    expect(eventWords({ kind: 'event', title: 'Heat Fissure', rewards: [] }).detail).toBe('In-game event.');
  });

  it('words alerts by their rewards, boosters by what they raise, and Nightwave by its phase', () => {
    expect(eventWords({ kind: 'alert', rewards: [{ item: 'Kuva', count: 200000 }, { item: 'Exilus Warframe Adapter', count: 1 }], credits: 10000 }))
      .toEqual({ title: 'Alert: Kuva ×200,000 and Exilus Warframe Adapter', short: 'Alert · Kuva ×200,000 and Exilus Warframe Adapter', detail: 'A mission open for a limited time. Play it before it ends to collect the reward and 10,000 credits.' });
    expect(eventWords({ kind: 'alert', rewards: [], credits: 5000 }).title).toBe('Alert: 5,000 credits');
    expect(eventWords({ kind: 'booster', boost: 'resource-amount', factor: 2 }).title).toBe('Resource drop amount ×2');
    expect(eventWords({ kind: 'booster', boost: 'other', factor: null }).title).toBe('Game-wide booster');
    expect(eventWords({ kind: 'nightwave', intermission: true }).title).toBe('Nightwave intermission');
    expect(eventWords({ kind: 'nightwave', intermission: false }).title).toBe('Nightwave season');
  });

  it('prefers an announcement’s own note, and falls back by its type', () => {
    expect(eventWords({ kind: 'official', title: 'Update 41', note: 'Patch notes follow at release.', type: 'update' }).detail).toBe('Patch notes follow at release.');
    expect(eventWords({ kind: 'official', title: 'Devstream 200', note: null, type: 'devstream' }).detail).toMatch(/^DE presents what is coming next/);
    expect(eventWords({ kind: 'official', title: 'X', note: null, type: 'unheard-of' }).detail).toBe('An announcement from DE.');
  });

  it('shows a Darvo price only when both parts are known', () => {
    expect(eventWords({ kind: 'deal', item: 'Tenno Sword Shield', discount: 90, price: 15 }).title).toBe('Darvo: Tenno Sword Shield (90% off, 15p)');
    expect(eventWords({ kind: 'deal', item: 'Tenno Sword Shield', discount: null, price: null }).title).toBe('Darvo: Tenno Sword Shield');
  });

  it('joins names with the language’s own list', async () => {
    await i18n.use('de');
    expect(eventWords({ kind: 'vault', names: ['Cedo Prime', 'Lavos Prime'] }).short).toContain('Cedo Prime und Lavos Prime');
  });
});
