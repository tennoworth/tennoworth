// Presentation helpers shared across the views. Everything here was written
// out two or more times before it moved; this file is not a home for
// one-line wrappers around a builtin.

/** Canonical warframe.market item page. Built by hand at four call sites,
 *  which is three chances to typo a URL nothing type-checks. */
export function wfmItemUrl(slug: string): string {
  return `https://warframe.market/items/${slug}`;
}

// warframestat's Baro feed usually carries display names ("Strata Relay
// (Earth)"), but event nodes leak internals — TennoConHUB2 rendered raw in the
// countdown. Map the known offenders, pass everything else through.
const NODE_NAMES: Record<string, string> = {
  TennoConHUB2: 'TennoCon Relay',
  SolarisUnitedHub1: 'Fortuna backroom',
};

/** Baro's location, with internal node names cleaned up. */
export function baroLocation(location: string): string {
  return NODE_NAMES[location] ?? location;
}

/** A duration as "3d 4h" / "4h 12m" / "12m". Null/negative/non-finite render
 *  as an em dash — the Baro feed goes absent during outages, and a countdown
 *  reading "NaNd NaNh" is worse than one that admits it doesn't know. */
export function humanWindow(ms: number | null | undefined): string {
  if (ms == null || !Number.isFinite(ms) || ms < 0) return '—';
  const totalMin = Math.floor(ms / 60000);
  const d = Math.floor(totalMin / (60 * 24));
  const h = Math.floor((totalMin / 60) % 24);
  const m = totalMin % 60;
  if (d > 0) return `${d}d ${h}h`;
  if (h > 0) return `${h}h ${m}m`;
  return `${m}m`;
}

/** Platinum, rounded and thousands-separated.
 *
 *  Exists because the listing review modal rendered the same quantity as
 *  `avg.toFixed(0)` while the table and market browser used
 *  `Math.round(v).toLocaleString()` — so a maxed arcane's average read "2500"
 *  in the modal and "2,500" everywhere else. Plat is always a whole number to
 *  the user; WFM has no fractional prices. */
export function plat(v: number | null | undefined): string {
  if (v == null || !Number.isFinite(v)) return '—';
  return Math.round(v).toLocaleString();
}

/** Split the copies a row holds back into "leveled" (untradeable in-game) and
 *  "kept" (the user's Keep-copies reserve).
 *
 *  The arithmetic and the two tooltips were written out in both ResultsTable
 *  and ListingReviewModal. Their MARKUP differs on purpose — the table shows a
 *  parenthesised suffix, the modal a "N owned · …" line — so only the parts
 *  that must agree moved here. Leveled is clamped to heldBack: a row can hold
 *  back fewer copies than it has leveled ones when the reserve is 0, and an
 *  unclamped subtraction renders a negative "kept". */
export function ownedBreakdown(
  owned: number,
  sellable: number,
  leveled: number | null | undefined,
): { heldBack: number; leveledPart: number; keptPart: number } {
  const heldBack = owned - sellable;
  const leveledPart = Math.min(leveled || 0, heldBack);
  return { heldBack, leveledPart, keptPart: heldBack - leveledPart };
}

export const LEVELED_NOTE_TITLE =
  "Leveled gear can't be traded in-game — only unranked copies can be sold.";

export function keptNoteTitle(keptPart: number): string {
  return `${keptPart} cop${keptPart === 1 ? 'y' : 'ies'} held back by the Keep copies reserve — not counted as sellable.`;
}

/**
 * Human label for a snapshot-freshness bucket. Single source for the
 * bucket → label map (the freshness dot's title and aria-label both use it),
 * and the explicit 'unknown' branch means an untimestamped snapshot never
 * reads as "over 24 hours old".
 */
export function freshnessLabel(f: 'fresh' | 'aging' | 'stale' | 'unknown'): string {
  if (f === 'fresh') return 'under 3 hours old';
  if (f === 'aging') return '3 to 24 hours old';
  if (f === 'stale') return 'over 24 hours old';
  return 'age unknown — no timestamp in this snapshot';
}
