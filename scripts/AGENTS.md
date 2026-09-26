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
- `check-probe-report.ts` - the gate for the TENNOWORTH_PROBE UI smoke run
  (ui-smoke.yml): asserts the probe's evidence JSON shows the app booted
  into Tauri IPC mode, the sell view rendered its scan CTA, and no
  console/CSP violations were logged - the failure class static gates
  cannot see.
- `check-agent-instructions.ts` - verifies that the agent instruction files'
  relative links resolve and that every tracked `*/AGENTS.md` is reachable from
  the root router. Run it from the repository root. It reads the git index, so a
  new instruction file must be staged before it is visible here, and it
  deliberately does not check backticked paths. Its optional `--local` mode
  checks installed skill frontmatter/links and local adapter instruction targets
  on disk; it is not a publication gate and requires no provider runtime.
- `check-public-surface.ts` - the gate for what may be published. It fails when
  maintainer-local material (research, plans, audits, host runbooks) is tracked
  despite the ignore rules, and when a tracked document under `docs/` is
  referenced from nowhere outside `docs/`. Run it from the repository root; it
  reads the git index only, so it needs no install step and runs on
  documentation-only changes. The rule it enforces is stated in ../AGENTS.md.
- `check-lxc-backup.sh` - the workstation-side preservation job for the
  observation corpus, which the box's readiness check reports as
  `external-backup`. It copies the newest `vzdump-lxc-<vmid>-*.tar.zst`
  and its `.log` off the Proxmox node, proves the copy on receipt (`zstd -t`, a
  `tar --zstd` listing, and the newest corpus log's first record read back out of
  the archive), records a manifest of what was verified and when, rotates older
  local archives beyond `KEEP`, and refuses to record or delete anything when
  verification fails. It is read-only on the node and runs from the maintainer's
  machine, not the box.

Script tests are `scripts/*.test.ts`, run with `bun test`; `tests/` holds only
the shared fixtures.
