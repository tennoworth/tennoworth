import { describe, expect, it } from 'vitest';
import { parseColumnChoice } from './filters.svelte';

describe('parseColumnChoice', () => {
  it('reads per-preset column lists', () => {
    expect(parseColumnChoice('{"default":["name","owned"],"custom":["name","ducats"]}'))
      .toEqual({ default: ['name', 'owned'], custom: ['name', 'ducats'] });
  });

  it('falls back to the presets for a missing, malformed or wrong-shaped value', () => {
    for (const raw of [null, '', 'not json', '[]', '"default"', 'null']) expect(parseColumnChoice(raw)).toEqual({});
  });

  it('drops entries that are not non-empty string lists and keeps the rest', () => {
    expect(parseColumnChoice('{"a":[],"b":"name","c":[1,2],"d":["name"]}')).toEqual({ d: ['name'] });
  });
});
