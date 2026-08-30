// Client-side mirrors of wfm-core's server-enforced caps. wfm-core
// is the source of truth - these exist only so the UI can reject an
// out-of-range value before round-tripping to it.
//
// Agreement is gated, not asked for: tests/fixtures/limits.json is asserted
// here (limits.test.ts) and in wfm-core's plan.rs. The comment this replaced
// said "keep in sync with … listing.rs", and had already drifted - only
// MAX_PLATINUM lives there; MAX_PLAN_ITEMS and MIN_PLATINUM are in plan.rs.

// WFM's own UI cap. 999 was tried first and was too conservative - it
// silently blocked listings for maxed Arcane Energize / Galvanized
// Aptitude etc. (real prices 1500-2500p).
export const MAX_PLATINUM = 3000;
export const MIN_PLATINUM = 5;
export const MAX_PLAN_ITEMS = 50;
