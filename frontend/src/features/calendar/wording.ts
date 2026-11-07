// An event's title, short label and description, in the reader's language.
import type { CalendarEvent, CalendarText } from '../../domain/calendar-events';
import type { FeedText } from '../../domain/calendar-feed';
import { joinNames } from '../../domain/calendar-events';
import { formatLocale, t } from '../../i18n/i18n.svelte';

const OFFICIAL = ['update', 'devstream', 'prime-access', 'drops', 'nightwave', 'ducats', 'event', 'other'] as const;

/** "A, B and C" in English as the calendar always wrote it; the language's own list elsewhere. */
export function listNames(names: string[]): string {
  const locale = formatLocale();
  return locale ? new Intl.ListFormat(locale, { type: 'conjunction' }).format(names) : joinNames(names);
}

export interface EventWords { title: string; short: string; detail: string }

export function eventWords(text: CalendarText): EventWords {
  switch (text.kind) {
    case 'baro': {
      const where = text.where ?? t('cal.baro.relay');
      return { title: t('cal.baro.title', { where }), short: t('cal.baro.short', { where }), detail: t('cal.baro.detail') };
    }
    case 'baro-estimate':
      return { title: t('cal.baroEst.title'), short: t('cal.baroEst.short'), detail: t('cal.baroEst.detail') };
    case 'resurgence': {
      const names = listNames(text.names);
      return { title: t('cal.resurgence.title', { names }), short: t('cal.resurgence.short', { names }), detail: t('cal.resurgence.detail', { names }) };
    }
    case 'vault': {
      const names = listNames(text.names);
      return { title: t('cal.vault.title', { names }), short: t('cal.vault.short', { names }), detail: t('cal.vault.detail') };
    }
    case 'deal':
      return {
        title: text.discount !== null && text.price !== null
          ? t('cal.deal.titlePriced', { item: text.item, discount: text.discount, price: text.price })
          : t('cal.deal.title', { item: text.item }),
        short: t('cal.deal.short', { item: text.item }),
        detail: t('cal.deal.detail'),
      };
    case 'event': {
      const rewards = listNames(text.rewards.slice(0, 3));
      const detail = !text.rewards.length ? t('cal.event.detail') : t(text.rewards.length > 3 ? 'cal.event.rewardsMore' : 'cal.event.rewards', { rewards });
      return { title: text.title, short: text.title, detail };
    }
    case 'official': {
      const type = (OFFICIAL as readonly string[]).includes(text.type) ? (text.type as (typeof OFFICIAL)[number]) : 'other';
      return { title: text.title, short: text.title, detail: text.note ?? t(`cal.official.${type}`) };
    }
    case 'given':
      return { title: text.title, short: text.title, detail: text.detail };
  }
}

export const wordsOf = (event: CalendarEvent): EventWords => eventWords(event.text);

/** A trader-calendar row's title and its detail after the dot, if any. */
export function feedWords(text: FeedText): { title: string; detail: string | null } {
  switch (text.kind) {
    case 'baro':
      return {
        title: text.where ? t('feed.baroAt', { where: text.where }) : t('feed.baro'),
        detail: text.stock ? t('feed.baroStock', { count: text.stock }) : t('feed.baroNoStock'),
      };
    case 'vault':
      return { title: t('feed.vault'), detail: t('feed.packs', { count: text.packs }) };
    case 'deal':
      return {
        title: t('feed.deal', { item: text.item }),
        detail: text.discount !== null && text.price !== null ? t('feed.dealPrice', { discount: text.discount, price: text.price }) : null,
      };
    case 'event': {
      if (text.coverage === 'unknown') return { title: text.title, detail: t('feed.rewardsUnknown') };
      const parts: string[] = [];
      if (text.rewards > 0) parts.push(t('feed.rewards', { count: text.rewards }));
      if (text.credits > 0) parts.push(t('feed.credits', { credits: text.credits }));
      if (text.coverage === 'partial') parts.push(t('feed.partial'));
      return { title: text.title, detail: parts.join(' · ') || null };
    }
  }
}
