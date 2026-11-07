/**
 * Trade presence in the user's words: warframe.market's three statuses and its
 * "keep status for" choices, under the names the website uses.
 */
import type { PresenceChoice, PresenceStatus } from '../../contracts/desktop';
import { formatLocale, t } from '../../i18n/i18n.svelte';

export const PRESENCE_CHOICES: readonly PresenceChoice[] = ['online', 'ingame', 'invisible'];

export function presenceLabel(choice: PresenceChoice): string {
  return t(`presence.${choice}`);
}

/** What other traders see, one line per choice. */
export function presenceHint(choice: PresenceChoice): string {
  return t(`presence.hint.${choice}`);
}

/** warframe.market's "while connected" is `null`: kept while TennoWorth runs. */
export function keepForLabel(minutes: number | null): string {
  if (minutes == null) return t('presence.whileRunning');
  return minutes % 60 === 0 ? t('presence.hours', { n: minutes / 60 }) : t('presence.minutes', { n: minutes });
}

type Clock = (iso: string) => string;
const clock: Clock = (iso) => new Date(iso).toLocaleTimeString(formatLocale(), { hour: '2-digit', minute: '2-digit' });

/** The status word, or what stands in for it before the server has answered. */
export function presenceWord(status: PresenceStatus): string {
  if (status.problem === 'not_verified') return t('presence.verify');
  if (status.status) return presenceLabel(status.status);
  return t(status.connected || status.problem === 'unreachable' ? 'presence.unknown' : 'presence.connecting');
}

/** Invisible while Warframe runs: buyers who filter for online sellers miss you. */
export function presenceHiddenWhilePlaying(status: PresenceStatus): boolean {
  return presenceUsable(status) && status.status === 'invisible' && status.gameRunning;
}

/**
 * What sits beside the status in the strip: the one-click fix while playing
 * invisible, the note that Follow the game holds the status, or the retry.
 */
export function presenceAside(status: PresenceStatus): 'go-online' | 'auto' | 'retrying' | null {
  if (status.problem === 'unreachable') return 'retrying';
  if (presenceHiddenWhilePlaying(status)) return 'go-online';
  return status.following && status.problem !== 'not_verified' ? 'auto' : null;
}

/** One line on how the status is held, for the menu and Settings. */
export function presenceLine(status: PresenceStatus, at: Clock = clock): string {
  if (status.problem === 'not_verified') return t('presence.line.notVerified');
  if (status.problem === 'paused') return t('presence.line.paused');
  if (!status.connected) return t(status.problem === 'unreachable' ? 'presence.line.unreachable' : 'presence.line.connecting');
  if (status.following) return t(status.gameRunning ? 'presence.line.followingRunning' : 'presence.line.followingClosed');
  if (status.followPaused) return t('presence.line.byHand');
  if (status.managed) return t('presence.line.managed');
  if (status.status === 'invisible') return t('presence.line.invisible');
  if (status.statusUntil) return t('presence.line.until', { time: at(status.statusUntil) });
  return t('presence.line.wfm');
}

/** A notice for a problem the user can act on, or null. */
export function presenceProblem(status: PresenceStatus): string | null {
  switch (status.problem) {
    case 'not_verified':
      return t('presence.problem.notVerified');
    case 'refused':
      return status.detail ? t('presence.problem.refusedDetail', { detail: status.detail }) : t('presence.problem.refused');
    case 'paused':
      return t('presence.problem.paused');
    case 'unreachable':
      return t('presence.problem.unreachable');
    default:
      return null;
  }
}

/** The mark beside the status word. */
export function presenceMark(status: PresenceStatus): PresenceChoice | 'warn' {
  return status.problem === 'not_verified' || !status.status ? 'warn' : status.status;
}

/** Status changes are possible: signed in, connected, and not refused for the account. */
export function presenceUsable(status: PresenceStatus | null): boolean {
  return !!status && status.signedIn && status.connected && status.problem !== 'not_verified';
}
