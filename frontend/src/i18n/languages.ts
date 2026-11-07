// The game's languages. Codes match warframe.market's and the reward overlay's
// (rust/tennoworth-desktop/src/overlay/language.rs); `tag` is the BCP 47 tag
// used for `lang`, plural rules and number formatting. `offered` languages are
// the ones a player can choose: the app and the reward overlay both work in
// them. Thai is not offered: warframe.market has no Thai item names, so its
// rewards would read in English. Its catalog stays in review meanwhile.
export const LANGUAGES = [
  { code: 'en', tag: 'en', native: 'English', english: 'English', offered: true },
  { code: 'de', tag: 'de', native: 'Deutsch', english: 'German', offered: true },
  { code: 'es', tag: 'es', native: 'Español', english: 'Spanish', offered: true },
  { code: 'fr', tag: 'fr', native: 'Français', english: 'French', offered: true },
  { code: 'it', tag: 'it', native: 'Italiano', english: 'Italian', offered: true },
  { code: 'pt', tag: 'pt-BR', native: 'Português', english: 'Portuguese (Brazil)', offered: true },
  { code: 'pl', tag: 'pl', native: 'Polski', english: 'Polish', offered: true },
  { code: 'ru', tag: 'ru', native: 'Русский', english: 'Russian', offered: true },
  { code: 'uk', tag: 'uk', native: 'Українська', english: 'Ukrainian', offered: true },
  { code: 'tr', tag: 'tr', native: 'Türkçe', english: 'Turkish', offered: true },
  { code: 'ja', tag: 'ja', native: '日本語', english: 'Japanese', offered: true },
  { code: 'ko', tag: 'ko', native: '한국어', english: 'Korean', offered: true },
  { code: 'zh-hans', tag: 'zh-Hans', native: '简体中文', english: 'Chinese (Simplified)', offered: true },
  { code: 'zh-hant', tag: 'zh-Hant', native: '繁體中文', english: 'Chinese (Traditional)', offered: true },
  { code: 'th', tag: 'th', native: 'ไทย', english: 'Thai', offered: false },
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
