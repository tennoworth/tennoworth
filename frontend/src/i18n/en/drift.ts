// Meta drift: DE's yearly equip-share snapshots compared.
import { m, type Source } from '../source';

export const DRIFT = {
  'drift.label': m('DE equip share · {prior} → {current}', 'Subtitle: the two years compared.'),
  'drift.labelMissing': m('DE equip share · {prior} → {current} ({missing} unavailable)', 'Subtitle; missing lists years DE did not publish.'),
  'drift.title': m('Meta Drift', 'View heading.'),
  'drift.changes': m('Equip-share changes', 'Section heading.'),
  'drift.intro': m('Annual DE equip-share snapshots, published in arrears. Deltas compare the same item within the same category; they are percentage points, not causes or forecasts.', 'Introduction.'),
  'drift.excluded': m('{count, plural, one {# category change was incomparable and excluded.} other {# category changes were incomparable and excluded.}}', 'Note.'),
  'drift.movement': m('Movement', 'Field label.'),
  'drift.gains': m('Gains', 'Choice.', 10),
  'drift.losses': m('Losses', 'Choice.', 10),
  'drift.only': m('Only in year data', 'Choice: items in one year’s data only.', 18),
  'drift.equipment': m('Equipment', 'Field label.'),
  'drift.search': m('Search equipment', 'Search placeholder.'),
  'drift.category': m('Category', 'Field label and column header.', 12),
  'drift.allCategories': m('All categories', 'Choice.'),
  'drift.col.name': m('Name', 'Column header.', 10),
  'drift.col.delta': m('Δ share', 'Column header: change in share.', 8),
  'drift.col.share': m('Share', 'Column header.', 8),
  'drift.noGains': m('No matching gains.', 'Empty state.'),
  'drift.noLosses': m('No matching losses.', 'Empty state.'),
  'drift.onlyIn': m('Only in {year} data', 'Table heading.'),
  'drift.noItems': m('No matching items.', 'Empty state.'),
} satisfies Record<string, Source>;
