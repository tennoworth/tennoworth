import { describe, expect, it } from 'vitest';

const files = import.meta.glob<string>(['../App.svelte', '../components/**/*.svelte'], { query: '?raw', import: 'default', eager: true });

describe('component design contract', () => {
  for (const [file, source] of Object.entries(files)) {
    it(`${file.split('/').at(-1)} uses shared color, type, and shape tokens`, () => {
      const css = (source.match(/<style[^>]*>([\s\S]*?)<\/style>/)?.[1] ?? '')
        .replace(/\/\*[\s\S]*?\*\//g, '');
      expect(css.match(/#[\da-f]{3,8}\b|\brgba?\(/gi), 'literal component colors belong in app.css').toBeNull();
      expect(css.match(/font-size:\s*[\d.]+px\b|font:\s*(?:(?:\d{3}|italic|normal|bold)\s+)*[\d.]+px\b/g), 'text sizes use semantic tokens').toBeNull();
      expect(css.match(/border-radius:\s*[1-9][\d.]*px\b/g), 'control and panel shapes use tokens; circular status marks may use percentages').toBeNull();
    });
  }
});
