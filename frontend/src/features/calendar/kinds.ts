import type { EventKind } from '../../domain/calendar-events';
import { t } from '../../i18n/i18n.svelte';

export const KINDS: EventKind[] = ['official', 'event', 'baro', 'resurgence', 'vault', 'nightwave', 'notice', 'routine', 'deal'];

/** Routines live on this device, so the hosted calendar never offers them. */
export const kindsFor = (surface: 'desktop' | 'hosted'): EventKind[] =>
  surface === 'desktop' ? KINDS : KINDS.filter(kind => kind !== 'routine');

/** Context key for the desktop's "open Routines" action, read by EventDetail. */
export const OPEN_ROUTINES = Symbol('open-routines');

export const kindLabel = (kind: EventKind): string => t(`cal.kind.${kind}`);

/** Darvo changes daily and never touches tradeable items, so it starts hidden. */
export const HIDDEN_BY_DEFAULT: EventKind[] = ['deal'];
