import { describe, expect, it } from 'bun:test';
import { findAttribution } from './check-attribution';

describe('findAttribution', () => {
  it('refuses tool co-author trailers and generated-with lines', () => {
    expect(findAttribution('fix: x\n\nCo-Authored-By: Claude Opus <noreply@anthropic.com>')).toEqual(['tool co-author trailer']);
    expect(findAttribution('feat: y\n\nco-authored-by: GitHub Copilot <copilot@github.com>')).toEqual(['tool co-author trailer']);
    expect(findAttribution('Summary\n\n🤖 Generated with [Claude Code](https://claude.com/claude-code)'))
      .toEqual(['generated-with line', 'assistant link', 'assistant marker']);
  });

  it('accepts human co-authors and prose about the product', () => {
    expect(findAttribution('fix: x\n\nCo-Authored-By: Ana Silva <ana@example.com>')).toEqual([]);
    expect(findAttribution('docs: explain how the AI advisor proxy was removed')).toEqual([]);
    expect(findAttribution('chore: generated bindings from Rust types')).toEqual([]);
  });
});
