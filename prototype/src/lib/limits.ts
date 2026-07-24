// Client-side mirrors of companion/wfm-core/src/listing.rs's server-enforced
// caps. The companion is the source of truth — these exist only so the UI
// can reject an out-of-range value before round-tripping to the companion.
// Keep in sync with MAX_PLAN_ITEMS / MIN_PLATINUM / MAX_PLATINUM there.

// WFM's own UI cap. 999 was tried first and was too conservative — it
// silently blocked listings for maxed Arcane Energize / Galvanized
// Aptitude etc. (real prices 1500-2500p).
export const MAX_PLATINUM = 3000;
export const MIN_PLATINUM = 5;
export const MAX_PLAN_ITEMS = 50;
