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
- `archive-observations.sh` - copies the host's observation logs to an off-box
  destination, gzipped, with a manifest that records each file's uncompressed
  and compressed hashes and the deployment observed at archive time, and a
  receipt the box pulls. It verifies every file after transfer, re-fetches a
  stored copy whose artifact or hashes no longer match, and exits non-zero on
  any mismatch without refreshing the receipt; read it before changing what it
  stores, because the replay evidence is only as good as the archive.
  `deploy/archive-observations.{service,timer}` schedule it on the archive host,
  and `install-archive-host.sh` installs those units, the alert template and the
  alert handler on that host and verifies them - `OnFailure=wfm-alert@%n.service`
  provisions nothing by itself. Before a receipt renews any claim, the script
  re-verifies every artifact the manifest names, not only the files the box still
  lists. `deploy/observations-check.sh` is its on-box counterpart and consumes
  the receipt `deploy/pull-archive-receipt.sh` fetches hourly in its
  `on-box-archive` mode; the check itself has no network access. The
  `external-backup` mode is the deployment that replaced it: the corpus is
  protected by host-level Proxmox backups of the whole container instead, so the
  check requires no receipt and reports preservation as declared-external and
  not independently verified from the box.
- `check-lxc-backup.sh` - the workstation-side preservation job for the
  `external-backup` deployment. It copies the newest `vzdump-lxc-<vmid>-*.tar.zst`
  and its `.log` off the Proxmox node, proves the copy on receipt (`zstd -t`, a
  `tar --zstd` listing, and the newest corpus log's first record read back out of
  the archive), records a manifest of what was verified and when, rotates older
  local archives beyond `KEEP`, and refuses to record or delete anything when
  verification fails. It is read-only on the node and runs from the maintainer's
  machine, not the box.

Tests live in `tests/` (Rust + TS suites; no pytest).
