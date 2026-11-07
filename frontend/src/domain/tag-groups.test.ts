import { describe, expect, it } from 'vitest';
import { canonicalTag, canonicalTags, tagGroup, topTags } from './tag-groups';
import { tagLabel, tagMatches } from '../desktop/selling/tag-labels';

describe('tag groups', () => {
  it('folds every breed into its species, and nothing else', () => {
    expect(canonicalTags(['mod', 'huras_kubrow', 'kubrow', 'smeeta_kavat', 'panzer_vulpaphyla'])).toEqual(['mod', 'kubrow', 'kavat', 'vulpaphyla']);
    expect(canonicalTag('kubrow_claws')).toBe('kubrow_claws');
    expect(canonicalTag('void key')).toBe('void_key');
  });

  it('sorts tags the way the game does, with names last', () => {
    expect(tagGroup('relic')).toBe('type');
    expect(tagGroup('nikanas')).toBe('slot');
    expect(tagGroup('rare')).toBe('rarity');
    expect(tagGroup('axi')).toBe('era');
    expect(tagGroup('sunika_kubrow')).toBe('companion');
    expect(tagGroup('nyx')).toBe('name');
    expect(tagGroup('ax_52')).toBe('name');
  });

  it('keeps rarity, names and the catch-all weapon off the main line', () => {
    const available = [['mod', 321], ['rare', 240], ['warframe', 127], ['weapon', 68], ['melee', 46], ['nyx', 40], ['relic', 16], ['empty', 0]] as const;
    expect(topTags(available)).toEqual(['mod', 'warframe', 'melee', 'relic']);
    expect(topTags(available, 2)).toEqual(['mod', 'warframe']);
  });

  it('reads category tags as words and names as the game writes them', () => {
    expect(tagLabel('thrown_melee')).toBe('thrown melee');
    expect(tagLabel('arcane_enhancement')).toBe('arcane');
    expect(tagLabel('jat_kittag')).toBe('Jat Kittag');
    expect(tagLabel('ax_52')).toBe('Ax-52');
    expect(tagMatches('ax_52', 'ax-5')).toBe(true);
    expect(tagMatches('jat_kittag', 'JAT kit')).toBe(true);
    expect(tagMatches('nyx', 'oberon')).toBe(false);
    expect(tagMatches('nyx', '  ')).toBe(true);
  });

  it('finds a name by the start of its words, in any order', () => {
    expect(tagMatches('jat_kittag', 'kit jat')).toBe(true);
    expect(tagMatches('jat_kittag', 'j k')).toBe(true);
    expect(tagMatches('two_handed_nikana', 'nik two')).toBe(true);
    expect(tagMatches('ax_52', '52 ax')).toBe(true);
    // Each typed word needs a word of its own, and must start it.
    expect(tagMatches('jat_kittag', 'jat jat')).toBe(false);
    expect(tagMatches('jat_kittag', 'at tag')).toBe(false);
    expect(tagMatches('jat_kittag', 'kitag')).toBe(false);
  });
});
