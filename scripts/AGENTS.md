# scripts/ - release, CSP, probe, and deployment tools

Run TypeScript tools with Bun. Shared cross-language fixtures live in
`tests/fixtures/`; the market pipeline lives in Rust. Use ../CONTRIBUTING.md
for required checks and ../docs/releasing.md for release preparation.

- `release.ts` - release preparation, snapshot validation, and bundled app notes;
  keep its contracts covered by `release.test.ts`.
- Deployment/layout helpers and their tests must preserve live data and follow
  ../docs/repository-layout-transition.md.
- `sync-csp.ts` - generates the Content-Security-Policy. The shipped copies,
  the allowed directives and the desktop build variant are documented once, in
  ../frontend/AGENTS.md; read that before changing anything here.
- `check-panic-sites.ts` - RETIRED 2026-08, replaced by the workspace clippy
  deny config. See rust/AGENTS.md for the rule.
- `check-probe-report.ts` - the gate for the TENNOWORTH_PROBE UI smoke run
  (ui-smoke.yml): asserts the probe's evidence JSON shows the app booted
  into Tauri IPC mode, the sell view rendered its scan CTA, and no
  console/CSP violations were logged - the failure class static gates
  cannot see.
- `check-agent-instructions.ts` - verifies that the agent instruction files'
  relative links resolve and that every tracked `*/AGENTS.md` is reachable from
  the root router. Run it from the repository root. It reads the git index, so a
  new instruction file must be staged before it is visible here, and it
  deliberately does not check backticked paths.

Tests live in `tests/` (Rust + TS suites; no pytest).
