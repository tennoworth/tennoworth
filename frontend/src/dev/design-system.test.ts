import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const files = import.meta.glob<string>(['../shells/**/*.svelte', '../features/**/*.svelte', '../ui/**/*.svelte', '../admin/**/*.svelte', './**/*.svelte'], { query: '?raw', import: 'default', eager: true });

describe('component design contract', () => {
  for (const [file, source] of Object.entries(files)) {
    it(`${file.split('/').at(-1)} uses shared color, type, and shape tokens`, () => {
      const css = (source.match(/<style[^>]*>([\s\S]*?)<\/style>/)?.[1] ?? '')
        .replace(/\/\*[\s\S]*?\*\//g, '');
      expect(css.match(/#[\da-f]{3,8}\b|\brgba?\(/gi), 'literal component colors belong in app.css').toBeNull();
      expect(css.match(/font-size:\s*[\d.]+px\b|font:\s*(?:(?:\d{3}|italic|normal|bold)\s+)*[\d.]+px\b/g), 'text sizes use semantic tokens').toBeNull();
      expect(css.match(/border-radius:\s*[1-9][\d.]*px\b/g), 'control and panel shapes use tokens; circular status marks may use percentages').toBeNull();
      const tagRules = [...css.matchAll(/([^{}]*\.tag\b[^{}]*)\{([^}]*)\}/g)].filter(([, , body]) => /(^|[;\s])(color|border(-color)?)\s*:/.test(body));
      expect(tagRules.map(([, selector]) => selector.trim()), 'tag colours live in the shared .tag vocabulary in app.css').toEqual([]);
    });
  }
});

// Vitest serves CSS imports empty, so the shared sheets are read from disk.
const sharedSheets = Object.fromEntries(['src/app.css', 'src/shells/shell.css'].map((path) => [path, readFileSync(path, 'utf8')]));

describe('table.tw row rules', () => {
  // yorha draws the table.tw row rule as a cell background image, so a fill
  // written with the background shorthand silently erases that row's rule.
  const sheets = [
    ...Object.entries(sharedSheets),
    ...Object.entries(files)
      .filter(([, source]) => /<table[^>]*class="[^"]*\btw\b/.test(source))
      .map(([file, source]) => [file, source.match(/<style[^>]*>([\s\S]*?)<\/style>/)?.[1] ?? ''] as const),
  ];
  for (const [file, source] of sheets) {
    it(`${file.split('/').at(-1)} fills table cells with background-color`, () => {
      const css = source.replace(/\/\*[\s\S]*?\*\//g, '');
      const shorthand = [...css.matchAll(/([^{}]+)\{([^{}]*)\}/g)]
        .filter(([, selector, body]) => selector.split(',').some((part) => /(^|[\s>+~])td\b[^\s>+~]*$/.test(part.trim())) && /(^|[;\s])background\s*:/.test(body))
        .map(([, selector]) => selector.trim());
      expect(shorthand, 'use background-color so the row rule survives').toEqual([]);
    });
  }
});
