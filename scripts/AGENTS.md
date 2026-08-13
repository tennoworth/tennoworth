# scripts/ — three TypeScript tools + the shared fixtures

Python was fully retired 2026-08: the scraper (`wfm_demand.py`), the converter
(`csv_to_market_json.py`) and the parity/diff tooling (`wfm_common.py`,
`semantic_diff.py`) are gone, replaced by the Rust `wfm-scrape` binary. What
remains in this directory is three TypeScript tools (run via `bun`); the
shared cross-language
fixtures live in `tests/fixtures/`.

- `sync-csp.ts` — the ONE source of truth for the Content-Security-Policy
  that ships in three hosted copies (`prototype/index.html` meta,
  `prototype/public/_headers`, `deploy/Caddyfile`) plus a desktop build
  variant. Edit the `DIRECTIVES` array, run `bun run csp` from `prototype/`
  to rewrite all three; `bun run build` fails via its prebuild `--check` if
  any copy drifted. Do NOT hand-edit the three copies.
- `check-panic-sites.ts` — the unwrap/expect gate (audit.yml). Fails on any
  new production `unwrap()`/`expect()` outside the ~8-site allowlist: a
  panic in a Tauri command is a crash plus a poisoned mutex, so unwraps are
  only tolerated where they are unreachable by construction. `--list` prints
  every production site for regenerating the allowlist after a refactor.
- `check-probe-report.ts` — the gate for the TENNOWORTH_PROBE UI smoke run
  (ui-smoke.yml): asserts the probe's evidence JSON shows the app booted
  into Tauri IPC mode, the sell view rendered its scan CTA, and no
  console/CSP violations were logged — the failure class static gates
  cannot see.

Tests live in `tests/` (Rust + TS suites; no pytest).
