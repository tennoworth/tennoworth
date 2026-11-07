// The game's languages. Codes match warframe.market's and the reward overlay's
// (rust/tennoworth-desktop/src/overlay/language.rs); `tag` is the BCP 47 tag
// used for `lang`, plural rules and number formatting. `offered` languages are
// the ones a player can choose: the app and the reward overlay both work in
// them. Thai is not offered: warframe.market has no Thai item names, so its
// rewards would read in English. Its catalog stays in review meanwhile.
// `word` is "Language" in that language: the first-run picker's title cycles
// through them before the catalogs it would load are chosen.
export const LANGUAGES = [
  { code: 'en', tag: 'en', native: 'English', english: 'English', word: 'Language', offered: true },
  { code: 'de', tag: 'de', native: 'Deutsch', english: 'German', word: 'Sprache', offered: true },
  { code: 'es', tag: 'es', native: 'Español', english: 'Spanish', word: 'Idioma', offered: true },
  { code: 'fr', tag: 'fr', native: 'Français', english: 'French', word: 'Langue', offered: true },
  { code: 'it', tag: 'it', native: 'Italiano', english: 'Italian', word: 'Lingua', offered: true },
  { code: 'pt', tag: 'pt-BR', native: 'Português', english: 'Portuguese (Brazil)', word: 'Idioma', offered: true },
  { code: 'pl', tag: 'pl', native: 'Polski', english: 'Polish', word: 'Język', offered: true },
  { code: 'ru', tag: 'ru', native: 'Русский', english: 'Russian', word: 'Язык', offered: true },
  { code: 'uk', tag: 'uk', native: 'Українська', english: 'Ukrainian', word: 'Мова', offered: true },
  { code: 'tr', tag: 'tr', native: 'Türkçe', english: 'Turkish', word: 'Dil', offered: true },
  { code: 'ja', tag: 'ja', native: '日本語', english: 'Japanese', word: '言語', offered: true },
  { code: 'ko', tag: 'ko', native: '한국어', english: 'Korean', word: '언어', offered: true },
  { code: 'zh-hans', tag: 'zh-Hans', native: '简体中文', english: 'Chinese (Simplified)', word: '语言', offered: true },
  { code: 'zh-hant', tag: 'zh-Hant', native: '繁體中文', english: 'Chinese (Traditional)', word: '語言', offered: true },
  { code: 'th', tag: 'th', native: 'ไทย', english: 'Thai', word: 'ภาษา', offered: false },
] as const;

export type LanguageCode = (typeof LANGUAGES)[number]['code'];

/** Every language but English has a catalog under ./locales. */
export const TRANSLATED = LANGUAGES.filter(language => language.code !== 'en').map(language => language.code);

/** Whether a player can choose this language. */
export function isOfferedLanguage(value: unknown): value is LanguageCode {
  return LANGUAGES.some(language => language.code === value && language.offered);
}

export function isLanguageCode(value: unknown): value is LanguageCode {
  return LANGUAGES.some(language => language.code === value);
}

export function languageTag(code: LanguageCode | 'pseudo'): string {
  return code === 'pseudo' ? 'en-XA' : LANGUAGES.find(language => language.code === code)!.tag;
}
