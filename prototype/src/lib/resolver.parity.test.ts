// @ts-nocheck - fixture-driven parity check; full TS shapes here would be busy-work.
//
// Cross-language parity gate for the path/name → slug GUESS fallbacks. The
// canonical logic lives here (resolver.ts); it is mirrored in Rust
// (companion/tennoworth-desktop/src/sellables.rs) so the desktop tray
// resolves the same slug the web app would for an item neither catalog has
// an exact hit for. Both languages check the SAME shared fixture
// (tests/fixtures/name-guess/cases.json) against the SAME expected output.
//
// If either implementation's behavior changes, this test (or the Rust one)
// fails until both are brought back into agreement - that's the gate.
import { describe, it, expect } from 'vitest';
import { slugGuess, pathNameGuess } from './resolver.js';
import fixture from '../../../tests/fixtures/name-guess/cases.json';

describe('slugGuess / pathNameGuess parity (TS canonical side)', () => {
  it.each(fixture.path_name_guess_cases)('pathNameGuess($path) -> $expected', (c) => {
    expect(pathNameGuess(c.path)).toBe(c.expected);
  });

  it.each(fixture.slug_guess_cases)('slugGuess($name) -> $expected', (c) => {
    expect(slugGuess(c.name)).toBe(c.expected);
  });
});
