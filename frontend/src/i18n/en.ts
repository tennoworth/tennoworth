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

export const EN = {
  ...FIRST_RUN,
  ...STRIP,
  ...FORMAT,
  ...SELL,
  ...TABLE,
} satisfies Record<string, Source>;

export type MessageKey = keyof typeof EN;
