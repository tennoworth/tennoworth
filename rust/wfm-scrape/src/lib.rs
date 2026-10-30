//! The host market pipeline - the only implementation: `scrape` sweeps WFM to CSV and an observation log, `build`
//! renders `market.json` and `wfstat-catalog.json`.
//!
//! GROUND RULES:
//! - The build is NOT a pure transform; its failure semantics are contract.
//!   Per-surface preserve-on-empty, partial merges that keep the prior stamp
//!   (per key for keyed surfaces), and file-level preservation of
//!   wfstat-catalog.json must survive verbatim - see `reconcile`.
//! - ONE injected clock everywhere time is read: `updated_at`, every
//!   `surface_fetched_at` stamp, AND the vaulting-soon derivation. Fixtures
//!   are only reproducible if no code path calls the system clock directly.
//! - Heuristics live in `market_domain::math`; this crate never re-implements them.
//! - Fixture regression tests (tests/ dir) shell the real binary against the
//!   frozen inputs in tests/fixtures/{scrape,build}/ - never
//!   byte-diff, never live endpoints outside the one opt-in liveness test.

pub mod clock;
pub mod coerce;
pub mod ingest;
pub mod observations;
pub mod pipeline;
pub mod scrape;
