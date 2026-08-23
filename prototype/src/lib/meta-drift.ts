import type { Market } from './types';

export interface UsageHistoryRow {
  name: string;
  category: string;
  share: number;
}

export interface DriftRow {
  slug: string;
  name: string;
  category: string;
  priorShare: number;
  currentShare: number;
  deltaPp: number;
  lowSell: number | null;
  volume48h: number | null;
}

export interface OnlyRow {
  slug: string;
  name: string;
  category: string;
  year: number;
  share: number;
  lowSell: number | null;
  volume48h: number | null;
}

export interface MetaDriftModel {
  priorYear: number;
  currentYear: number;
  label: string;
  gains: DriftRow[];
  losses: DriftRow[];
  onlyPrior: OnlyRow[];
  onlyCurrent: OnlyRow[];
  categoryChanges: number;
  categories: string[];
}

function validRow(value: unknown): UsageHistoryRow | null {
  if (!value || typeof value !== 'object') return null;
  const row = value as Partial<UsageHistoryRow>;
  if (typeof row.name !== 'string' || !row.name
      || typeof row.category !== 'string' || !row.category
      || typeof row.share !== 'number' || !Number.isFinite(row.share) || row.share < 0) return null;
  return { name: row.name, category: row.category, share: row.share };
}

function marketFields(market: Market, slug: string): { lowSell: number | null; volume48h: number | null } {
  const item = market.items?.[slug];
  return {
    lowSell: typeof item?.low_sell === 'number' && Number.isFinite(item.low_sell) && item.low_sell >= 0 ? item.low_sell : null,
    volume48h: typeof item?.vol === 'number' && Number.isFinite(item.vol) && item.vol >= 0 ? item.vol : null,
  };
}

export function formatDeltaPp(value: number): string {
  const decimals = Math.abs(value) < 0.01 ? 4 : 2;
  return `${value > 0 ? '+' : ''}${value.toFixed(decimals)} pp`;
}

function availableYears(market: Market): Array<{ year: number; rows: Record<string, unknown> }> {
  const history = market.usage_history;
  if (!history?.by_year || typeof history.by_year !== 'object') return [];
  const candidates = new Set<number>();
  for (const value of history.years ?? []) {
    if (Number.isInteger(value) && value > 0) candidates.add(value);
  }
  return [...candidates].sort((a, b) => a - b).flatMap((year) => {
    const raw = history.by_year[String(year)] ?? history.by_year[year];
    if (!raw || typeof raw !== 'object' || Array.isArray(raw)) return [];
    const valid = Object.values(raw).some((row) => validRow(row));
    return valid ? [{ year, rows: raw as Record<string, unknown> }] : [];
  });
}

export function metaDriftLabel(priorYear: number, currentYear: number): string {
  const missing = [];
  for (let year = priorYear + 1; year < currentYear; year++) missing.push(year);
  return `DE equip share · ${priorYear} → ${currentYear}${missing.length ? ` (${missing.join(', ')} unavailable)` : ''}`;
}

export function buildMetaDrift(market: Market | null | undefined): MetaDriftModel | null {
  if (!market) return null;
  const available = availableYears(market);
  if (available.length < 2) return null;
  const prior = available.at(-2)!;
  const current = available.at(-1)!;
  const gains: DriftRow[] = [];
  const losses: DriftRow[] = [];
  const onlyPrior: OnlyRow[] = [];
  const onlyCurrent: OnlyRow[] = [];
  let categoryChanges = 0;
  const slugs = new Set([...Object.keys(prior.rows), ...Object.keys(current.rows)]);
  for (const slug of slugs) {
    const before = validRow(prior.rows[slug]);
    const after = validRow(current.rows[slug]);
    const fields = marketFields(market, slug);
    if (!before && !after) continue;
    if (!before && after) {
      onlyCurrent.push({ slug, ...after, year: current.year, ...fields });
      continue;
    }
    if (before && !after) {
      onlyPrior.push({ slug, ...before, year: prior.year, ...fields });
      continue;
    }
    if (before!.category !== after!.category) {
      categoryChanges++;
      continue;
    }
    const row: DriftRow = {
      slug,
      name: after!.name,
      category: after!.category,
      priorShare: before!.share,
      currentShare: after!.share,
      deltaPp: after!.share - before!.share,
      ...fields,
    };
    if (row.deltaPp > 0) gains.push(row);
    else if (row.deltaPp < 0) losses.push(row);
  }
  const tie = (a: { slug: string; name: string }, b: { slug: string; name: string }) =>
    a.slug.localeCompare(b.slug) || a.name.localeCompare(b.name);
  gains.sort((a, b) => b.deltaPp - a.deltaPp || tie(a, b));
  losses.sort((a, b) => a.deltaPp - b.deltaPp || tie(a, b));
  onlyCurrent.sort(tie);
  onlyPrior.sort(tie);
  const categories = [...new Set([
    ...gains.map((row) => row.category), ...losses.map((row) => row.category),
    ...onlyCurrent.map((row) => row.category), ...onlyPrior.map((row) => row.category),
  ])].sort((a, b) => a.localeCompare(b));
  return {
    priorYear: prior.year,
    currentYear: current.year,
    label: metaDriftLabel(prior.year, current.year),
    gains, losses, onlyPrior, onlyCurrent, categoryChanges, categories,
  };
}
