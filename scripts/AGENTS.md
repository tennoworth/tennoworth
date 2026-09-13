# scripts/ - release, CSP, probe, and deployment tools

Run TypeScript tools with Bun. Shared cross-language fixtures live in
`tests/fixtures/`; the market pipeline lives in Rust. Use ../CONTRIBUTING.md
for required checks and ../docs/releasing.md for release preparation.

- `release.ts` - release preparation, snapshot validation, and bundled app notes;
  keep its contracts covered by `release.test.ts`.
- Deployment/layout helpers and their tests must preserve live data and follow
  ../docs/repository-layout-transition.md.
- `sync-csp.ts` - the ONE source of truth for the Content-Security-Policy
  that ships in three hosted copies (`frontend/index.html` meta,
  `frontend/public/_headers`, `deploy/Caddyfile`) plus a desktop build
  variant. Edit the `DIRECTIVES` array, run `bun run csp` from `frontend/`
  to rewrite all three; `bun run build` fails via its prebuild `--check` if
  any copy drifted. Do NOT hand-edit the three copies.
- `check-panic-sites.ts` - RETIRED 2026-08, replaced by the workspace
  clippy deny config (rust/Cargo.toml `[workspace.lints.clippy]` +
  clippy.toml). The ~8 allowlist sites now live as `#[allow(<lint>, reason)]`
  attributes next to the code; the compiler-native gate is semantics-aware and
  additionally caught pacing panics (Duration subtraction) the grep could not
  see. See rust/AGENTS.md for the rule.
- `check-probe-report.ts` - the gate for the TENNOWORTH_PROBE UI smoke run
  (ui-smoke.yml): asserts the probe's evidence JSON shows the app booted
  into Tauri IPC mode, the sell view rendered its scan CTA, and no
  console/CSP violations were logged - the failure class static gates
  cannot see.

Tests live in `tests/` (Rust + TS suites; no pytest).
