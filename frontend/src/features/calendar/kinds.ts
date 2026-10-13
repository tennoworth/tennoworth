import type { EventKind } from '../../domain/calendar-events';

export const KIND_LABEL: Record<EventKind, string> = {
  event: 'DE events',
  baro: "Baro Ki'Teer",
  resurgence: 'Prime Resurgence',
  vault: 'Vaulting',
  notice: 'TennoWorth notices',
  deal: 'Darvo deals',
};

export const KINDS = Object.keys(KIND_LABEL) as EventKind[];

/** Darvo changes daily and never touches tradeable items, so it starts hidden. */
export const HIDDEN_BY_DEFAULT: EventKind[] = ['deal'];
