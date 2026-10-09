/**
 * Trade presence in the user's words: warframe.market's three statuses and its
 * "keep status for" choices, under the names the website uses.
 */
import type { PresenceChoice, PresenceStatus } from '../../contracts/desktop';

export const PRESENCE_CHOICES: readonly PresenceChoice[] = ['online', 'ingame', 'invisible'];

export const PRESENCE_LABEL: Record<PresenceChoice, string> = {
  online: 'Online',
  ingame: 'Online in game',
  invisible: 'Invisible',
};

/** What other traders see, one line per choice. */
export const PRESENCE_HINT: Record<PresenceChoice, string> = {
  online: 'Shown as online. Buyers can whisper you.',
  ingame: 'Buyers can whisper and invite you now.',
  invisible: 'Others see you as Offline. Your orders stay up, but most buyers filter for online sellers.',
};

/** warframe.market's "while connected" is `null`: kept while TennoWorth runs. */
export function keepForLabel(minutes: number | null): string {
  if (minutes == null) return 'While running';
  return minutes % 60 === 0 ? `${minutes / 60}h` : `${minutes}m`;
}

type Clock = (iso: string) => string;
const clock: Clock = (iso) => new Date(iso).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });

/** The status word, or what stands in for it before the server has answered. */
export function presenceWord(status: PresenceStatus): string {
  if (status.problem === 'not_verified') return 'Refused';
  if (status.status) return PRESENCE_LABEL[status.status];
  return status.connected ? 'Unknown' : 'Connecting…';
}

/** One line on how the status is held, for the menu and Settings. */
export function presenceLine(status: PresenceStatus, at: Clock = clock): string {
  if (status.problem === 'not_verified') return 'warframe.market refused: account not verified';
  if (status.problem === 'paused') return 'warframe.market access is paused';
  if (!status.connected) return status.problem === 'unreachable' ? 'Status channel unreachable · retrying' : 'Connecting to warframe.market…';
  if (status.following) return status.gameRunning ? 'Following the game · Warframe running' : 'Following the game · Warframe closed';
  if (status.followPaused) return 'Set by hand · following resumes next game session';
  if (status.managed) return 'Kept while TennoWorth runs';
  if (status.status === 'invisible') return 'Others see you as Offline';
  if (status.statusUntil) return `Until ${at(status.statusUntil)}`;
  return 'Set on warframe.market';
}

/** A notice for a problem the user can act on, or null. */
export function presenceProblem(status: PresenceStatus): string | null {
  switch (status.problem) {
    case 'not_verified':
      return 'warframe.market refused the change: this account is not verified. Verify it on warframe.market, then choose a status again.';
    case 'refused':
      return `warframe.market refused the change${status.detail ? ` (${status.detail})` : ''}.`;
    case 'paused':
      return 'warframe.market access is paused by the access policy or a cooldown. Your status stays as it was until access resumes.';
    case 'unreachable':
      return 'TennoWorth cannot reach warframe.market’s status channel right now. It retries on its own.';
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
