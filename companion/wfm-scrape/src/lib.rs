//! Rust port of the market pipeline's converter stage - the only
//! implementation (Python retired 2026-08).
//!
//! GROUND RULES:
//! - The converter is NOT a pure transform; its failure semantics are
//!   contract. Per-surface preserve-on-empty, partial-merge with a
//!   whole-surface NOW stamp, and file-level preservation of
//!   wfstat-catalog.json must survive verbatim - see `reconcile`.
//! - ONE injected clock everywhere time is read: `updated_at`, every
//!   `surface_fetched_at` stamp, AND the vaulting-soon derivation. Fixtures
//!   are only reproducible if no code path calls the system clock directly.
//! - Heuristics live in `market-math`; this crate never re-implements them.
//! - Fixture regression tests (tests/ dir) shell the real binary against the
//!   frozen inputs in tests/fixtures/convert/ - never byte-diff, never live
//!   endpoints.

pub mod clock;
pub mod coerce;
pub mod de;
pub mod de_extract;
pub mod csvin;
pub mod fetch;
pub mod history;
pub mod http;
pub mod orders;
pub mod reconcile;
pub mod render;
pub mod scrape;
pub mod stats;
