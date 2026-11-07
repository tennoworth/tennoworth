// Times, durations and counts shared across views (ui/format.ts).
import { m, type Source } from '../source';

export const FORMAT = {
  'format.justNow': m('just now', 'An age under a minute. Reads after a word like "updated", "Market", "recorded".', 12),
  'format.minutesAgo': m('{n} min ago', 'An age in minutes, e.g. "5 min ago". Reads after a word like "updated" or "Market".', 14),
  'format.hoursAgo': m('{n} h ago', 'An age in hours, e.g. "13 h ago". Keep it short: it sits in the status bar.', 12),
  'format.daysAgo': m('{n} d ago', 'An age in days, e.g. "3 d ago".', 12),
  'format.daysHours': m('{d}d {h}h', 'A time left or a duration in days and hours, e.g. "1d 2h" until Baro arrives. Use the shortest unit marks your language uses.', 10),
  'format.hoursMinutes': m('{h}h {m}m', 'A duration in hours and minutes, e.g. "4h 12m".', 10),
  'format.days': m('{d}d', 'A duration in whole days, e.g. "3d".', 6),
  'format.hours': m('{h}h', 'A duration in whole hours, e.g. "12h".', 6),
  'format.minutes': m('{m}m', 'A duration in minutes, e.g. "12m".', 6),
  'format.instant': m('instant', 'A foundry build time of zero.', 12),
  'format.today': m('Today', 'Heading over notifications from today.'),
  'format.yesterday': m('Yesterday', 'Heading over notifications from yesterday.'),
  'format.unknownDate': m('Unknown date', 'Heading over notifications whose date could not be read.'),
  'format.unknownTime': m('Unknown time', 'Shown in place of a time that could not be read.'),
  'format.leveledNote': m("Leveled gear can't be traded in-game - only unranked copies can be sold.", 'Tooltip on copies held back because they are leveled (ranked up).'),
  'format.keptNote': m('{count, plural, one {# copy held back by the Keep copies reserve - not counted as sellable.} other {# copies held back by the Keep copies reserve - not counted as sellable.}}', '"Keep copies" is a setting: how many copies of each item the player keeps instead of selling.'),
} satisfies Record<string, Source>;
