// The game's languages. Codes match warframe.market's and the reward overlay's
// (rust/tennoworth-desktop/src/overlay/language.rs); `tag` is the BCP 47 tag
// used for `lang`, plural rules and number formatting.
export const LANGUAGES = [
  { code: 'en', tag: 'en', native: 'English', english: 'English' },
  { code: 'de', tag: 'de', native: 'Deutsch', english: 'German' },
  { code: 'es', tag: 'es', native: 'Español', english: 'Spanish' },
  { code: 'fr', tag: 'fr', native: 'Français', english: 'French' },
  { code: 'it', tag: 'it', native: 'Italiano', english: 'Italian' },
  { code: 'pt', tag: 'pt-BR', native: 'Português', english: 'Portuguese (Brazil)' },
  { code: 'pl', tag: 'pl', native: 'Polski', english: 'Polish' },
  { code: 'ru', tag: 'ru', native: 'Русский', english: 'Russian' },
  { code: 'uk', tag: 'uk', native: 'Українська', english: 'Ukrainian' },
  { code: 'tr', tag: 'tr', native: 'Türkçe', english: 'Turkish' },
  { code: 'ja', tag: 'ja', native: '日本語', english: 'Japanese' },
  { code: 'ko', tag: 'ko', native: '한국어', english: 'Korean' },
  { code: 'zh-hans', tag: 'zh-Hans', native: '简体中文', english: 'Chinese (Simplified)' },
  { code: 'zh-hant', tag: 'zh-Hant', native: '繁體中文', english: 'Chinese (Traditional)' },
  { code: 'th', tag: 'th', native: 'ไทย', english: 'Thai' },
] as const;

export type LanguageCode = (typeof LANGUAGES)[number]['code'];

/** Every language but English has a catalog under ./locales. */
export const TRANSLATED = LANGUAGES.filter(language => language.code !== 'en').map(language => language.code);

export function isLanguageCode(value: unknown): value is LanguageCode {
  return LANGUAGES.some(language => language.code === value);
}

export function languageTag(code: LanguageCode | 'pseudo'): string {
  return code === 'pseudo' ? 'en-XA' : LANGUAGES.find(language => language.code === code)!.tag;
}
