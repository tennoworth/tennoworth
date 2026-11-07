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
