// Over the year to 2026-11, Saturday and Sunday each carried 16.5-16.7% of a
// week's trades against 13.3-13.4% Monday to Thursday: about a quarter more
// buyers. Prices moved under 1% by weekday, so this is about selling sooner,
// not for more. Thursday and Friday say list before it; the weekend says now.
export type WeekendNudge = 'before' | 'during';

export function weekendNudge(now: Date): WeekendNudge | null {
  const day = now.getDay();
  if (day === 4 || day === 5) return 'before';
  if (day === 6 || day === 0) return 'during';
  return null;
}
