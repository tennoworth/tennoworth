// The first-run language picker and the App language setting.
import { m, type Source } from '../source';

export const LANGUAGE = {
  'lang.lead': m('Which language should TennoWorth speak?', 'First-run language picker, the question. Shown in the language the cursor is on.'),
  'lang.sub': m('Relic reward names are read in it too. Change either one any time in Settings.', 'First-run language picker, under the question.'),
  'lang.continue': m('Continue', 'First-run language picker, the button that confirms the choice.', 16),
  'lang.yourGame': m('Your game', 'Tag on the language the game itself runs in.', 12),
  'lang.keys': m('<kbd>←</kbd><kbd>→</kbd> choose · <kbd>Enter</kbd> continue', 'Keyboard hint under the language list. Keep the keys in <kbd>.'),
  'lang.saveFailed': m('Could not save the language, so TennoWorth will ask again next time.', 'Error after choosing a language.'),
  'settings.appLanguage': m('App language', 'Setting.'),
  'settings.appLanguageHelp': m('TennoWorth’s own words. Item names stay as warframe.market lists them in English; the relic overlay has its own reward language.', 'Setting help.'),
  'settings.appLanguageFailed': m('Could not switch the language. Try again.', 'Error.'),
  'settings.rewardLanguage': m('Reward name language', 'Setting: which language the relic overlay reads reward names in.'),
  'settings.rewardLanguageHelp': m('Same as the game follows the language EE.log reports. A language other than English downloads its 1–5 MB text model once.', 'Setting help. Keep EE.log.'),
  'settings.rewardLanguageNow': m('Reading {language} names.', 'Status under the setting; language is a language name.'),
  'settings.sameAsGame': m('Same as the game', 'Choice: follow the game’s language.'),
} satisfies Record<string, Source>;
