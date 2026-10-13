// Event times are shown in UTC, as DE announces them, with the reader's own
// time beside it where a single moment matters.

export const DAY = 86_400_000;

export function utcDay(ms: number, long = false): string {
  return new Date(ms).toLocaleDateString('en-GB', long
    ? { weekday: 'long', day: 'numeric', month: 'long', timeZone: 'UTC' }
    : { weekday: 'short', day: 'numeric', month: 'short', timeZone: 'UTC' });
}

export function utcTime(ms: number): string {
  return new Date(ms).toLocaleTimeString('en-GB', { hour: '2-digit', minute: '2-digit', timeZone: 'UTC' });
}

export function localTime(ms: number): string {
  return new Date(ms).toLocaleString(undefined, { weekday: 'short', hour: '2-digit', minute: '2-digit' });
}

export function dayStart(ms: number): number {
  const d = new Date(ms);
  return Date.UTC(d.getUTCFullYear(), d.getUTCMonth(), d.getUTCDate());
}

/** Monday 00:00 UTC of the week containing `ms`. */
export function weekStart(ms: number): number {
  const day = dayStart(ms);
  return day - ((new Date(day).getUTCDay() + 6) % 7) * DAY;
}

export function countdown(ms: number): string {
  const days = Math.floor(ms / DAY);
  const hours = Math.floor((ms % DAY) / 3_600_000);
  if (days > 0) return `${days}d ${String(hours).padStart(2, '0')}h`;
  return `${hours}h ${String(Math.floor((ms % 3_600_000) / 60_000)).padStart(2, '0')}m`;
}
