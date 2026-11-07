// The listing-outcome record in words: rows for each group of listings and the
// one-line headline. Unknowns read as a dash, never as zero.
import type { ListingOutcomes, OutcomeGroup } from '../../contracts/desktop';
import type { WithinDeadline } from '../../contracts/generated/desktop';
import { formatLocale, t } from '../../i18n/i18n.svelte';

export interface OutcomeMeter {
  label: string;
  value: number;
  /** 0 while no listing is old enough to judge; the text then says so. */
  max: number;
  text: string;
}

export interface OutcomeRow {
  label: string;
  meters: OutcomeMeter[];
  listings: number;
  wait: string;
  price: string;
  priceTitle: string;
}

function wait(hours: number | null): string {
  if (hours === null) return '-';
  if (hours < 1) return t('ledger.waitUnderHour');
  if (hours < 48) return t('ledger.waitHours', { n: Math.round(hours) });
  return t('ledger.waitDays', { n: (hours / 24).toLocaleString(formatLocale(), { minimumFractionDigits: 1, maximumFractionDigits: 1 }) });
}

/** A deadline share is only shown once some listing is old enough to judge. */
function share(label: string, d: WithinDeadline): OutcomeMeter {
  return { label, value: d.sold, max: d.eligible, text: d.eligible ? t('ledger.xOfY', { x: d.sold, y: d.eligible }) : t('ledger.tooRecent') };
}

/** Rows for the groups that have listings; an empty group is not a 0% row. */
export function outcomeRows(o: ListingOutcomes): OutcomeRow[] {
  const groups: [string, OutcomeGroup][] = [[t('ledger.group.session'), o.session], [t('ledger.group.other'), o.manual]];
  return groups
    .filter(([, g]) => g.listings > 0)
    .map(([label, g]) => ({
      label,
      meters: [
        { label: t('ledger.copiesSold'), value: g.quantity_sold, max: g.quantity_listed, text: t('ledger.xOfY', { x: g.quantity_sold, y: g.quantity_listed }) },
        share(t('ledger.soldInDay'), g.within_day),
        share(t('ledger.soldInWeek'), g.within_week),
      ],
      listings: g.listings,
      wait: wait(g.median_hours_to_first_sale),
      price: g.price_ratio === null ? '-' : `${Math.round(g.price_ratio * 100)}%`,
      priceTitle: g.price_ratio === null ? t('ledger.noUnitPrice') : t('ledger.realisedOver', { count: g.priced_quantity }),
    }));
}

export function outcomeHeadline(o: ListingOutcomes): string {
  if (!o.enabled) return t('ledger.headline.off');
  const listings = o.session.listings + o.manual.listings;
  if (!listings) return t('ledger.headline.none', { days: o.window_days });
  const sold = [o.session, o.manual].reduce((n, g) => n + g.within_week.sold, 0);
  const judged = [o.session, o.manual].reduce((n, g) => n + g.within_week.eligible, 0);
  return judged
    ? t('ledger.headline.judged', { count: listings, days: o.window_days, sold, judged })
    : t('ledger.headline.recent', { count: listings, days: o.window_days });
}
