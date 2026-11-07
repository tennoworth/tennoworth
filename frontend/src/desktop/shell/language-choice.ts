// What one first-run language choice sets.
import { OVERLAY_LANGUAGES, type OverlayLanguage } from '../../contracts/data';

/** The relic overlay's reward language for an app language: following the
 *  game when they agree (or the game is unknown and the choice is English), the
 *  chosen language otherwise, so one choice covers both. */
export function rewardLanguageFor(appLanguage: string, gameLanguage: string | null): OverlayLanguage {
  if (appLanguage === gameLanguage || (gameLanguage === null && appLanguage === 'en')) return 'auto';
  return (OVERLAY_LANGUAGES as readonly string[]).includes(appLanguage) ? (appLanguage as OverlayLanguage) : 'auto';
}
