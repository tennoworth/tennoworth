// Client-side mirrors of the native listing caps, which are the source of truth
// (rust/market-domain/src/limits.rs). These exist only so the UI can reject an
// out-of-range value before the round trip.
//
// Agreement is gated, not asked for: tests/fixtures/limits.json is asserted
// here (limits.test.ts) and beside the Rust constants.

// WFM's own UI cap. 999 was tried first and was too conservative - it
// silently blocked listings for maxed Arcane Energize / Galvanized
// Aptitude etc. (real prices 1500-2500p).
export const MAX_PLATINUM = 3000;
export const MIN_PLATINUM = 5;
export const MAX_PLAN_ITEMS = 50;
