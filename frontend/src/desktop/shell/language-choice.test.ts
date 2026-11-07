import { expect, it } from 'vitest';
import { rewardLanguageFor } from './language-choice';

it('follows the game when the choice matches it, and reads the choice otherwise', () => {
  expect(rewardLanguageFor('pt', 'pt')).toBe('auto');
  expect(rewardLanguageFor('en', null)).toBe('auto');
  expect(rewardLanguageFor('de', 'en')).toBe('de');
  expect(rewardLanguageFor('de', null)).toBe('de');
  expect(rewardLanguageFor('en', 'ko')).toBe('en');
});
