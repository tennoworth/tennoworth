//! Rust port of the market pipeline's converter stage
//! (`scripts/csv_to_market_json.py`) — phase 3 of the Python→Rust
//! consolidation.
//!
//! GROUND RULES:
//! - The Python converter is NOT a pure transform; its failure semantics are
//!   contract. Per-surface preserve-on-empty, partial-merge with a
//!   whole-surface NOW stamp, and file-level preservation of
//!   wfstat-catalog.json must survive verbatim — see `reconcile`.
//! - ONE injected clock everywhere time is read: `updated_at`, every
//!   `surface_fetched_at` stamp, AND the vaulting-soon derivation. Fixtures
//!   are only reproducible if no code path calls the system clock directly.
//! - Heuristics live in `market-math`; this crate never re-implements them.
//! - Validation is a canonicalized semantic diff against the Python converter
//!   on the same frozen inputs (tests/fixtures/convert/), never byte-diff,
//!   never live endpoints.

pub mod clock;
pub mod csvin;
pub mod fetch;
pub mod jsonutil;
pub mod reconcile;
pub mod render;
