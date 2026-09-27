//! The host market pipeline - the only implementation (Python retired
//! 2026-08): `scrape` sweeps WFM to CSV and an observation log, `build`
//! renders `market.json` and `wfstat-catalog.json`, and `replay` reads the
//! observation logs back.
//!
//! GROUND RULES:
//! - The build is NOT a pure transform; its failure semantics are contract.
//!   Per-surface preserve-on-empty, partial merges that keep the prior stamp
//!   (per key for keyed surfaces), and file-level preservation of
//!   wfstat-catalog.json must survive verbatim - see `reconcile`.
//! - ONE injected clock everywhere time is read: `updated_at`, every
//!   `surface_fetched_at` stamp, AND the vaulting-soon derivation. Fixtures
//!   are only reproducible if no code path calls the system clock directly.
//! - Heuristics live in `market-math`; this crate never re-implements them.
//! - Fixture regression tests (tests/ dir) shell the real binary against the
//!   frozen inputs in tests/fixtures/{scrape,convert,replay}/ - never
//!   byte-diff, never live endpoints outside the one opt-in liveness test.

pub mod clock;
pub mod coerce;
pub mod csvin;
pub mod de;
pub mod de_extract;
pub mod history;
pub mod http;
pub mod ingest;
pub mod observations;
pub mod orders;
pub mod pipeline;
pub mod reconcile;
pub mod render;
pub mod replay;
pub mod scrape;
pub mod stats;
