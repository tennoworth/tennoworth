// The English source of every translatable message, one module per surface
// under ./en; a note tells translators where the text appears and any limit.
// Keys are `surface.name`. After changing a module run
// `bun scripts/i18n.ts --write` in frontend/ so every catalog gains, drafts or
// drops the key.
import type { Source } from './source';
import { FIRST_RUN } from './en/first-run';
import { STRIP } from './en/strip';
import { FORMAT } from './en/format';
import { SELL } from './en/sell';
import { TABLE } from './en/table';
import { REVIEW } from './en/review';
import { KEEP } from './en/keep';
import { SESSION } from './en/session';
import { SETS } from './en/sets';
import { LISTING } from './en/listing';
import { BUYERS } from './en/buyers';
import { SHELL } from './en/shell';
import { UI } from './en/ui';
import { ORDERS } from './en/orders';
import { BACKUP } from './en/backup';
import { LEDGER } from './en/ledger';
import { WATCHES } from './en/watches';
import { SETTINGS } from './en/settings';
import { PRIVACY } from './en/privacy';
import { NOTIFICATIONS } from './en/notifications';
import { ACCOUNT } from './en/account';
import { RIVENS } from './en/rivens';

export const EN = {
  ...FIRST_RUN,
  ...STRIP,
  ...FORMAT,
  ...SELL,
  ...TABLE,
  ...REVIEW,
  ...KEEP,
  ...SESSION,
  ...SETS,
  ...LISTING,
  ...BUYERS,
  ...SHELL,
  ...UI,
  ...ORDERS,
  ...BACKUP,
  ...LEDGER,
  ...WATCHES,
  ...SETTINGS,
  ...PRIVACY,
  ...NOTIFICATIONS,
  ...ACCOUNT,
  ...RIVENS,
} satisfies Record<string, Source>;

export type MessageKey = keyof typeof EN;
