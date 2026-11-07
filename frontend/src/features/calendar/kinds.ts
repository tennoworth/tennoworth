import type { EventKind } from '../../domain/calendar-events';
import { t } from '../../i18n/i18n.svelte';

export const KINDS: EventKind[] = ['official', 'event', 'baro', 'resurgence', 'vault', 'notice', 'deal'];

export const kindLabel = (kind: EventKind): string => t(`cal.kind.${kind}`);

/** Darvo changes daily and never touches tradeable items, so it starts hidden. */
export const HIDDEN_BY_DEFAULT: EventKind[] = ['deal'];
