# scripts/ - release, CSP, probe, and deployment tools

Run TypeScript tools with Bun from the repository root. Script tests are
`scripts/*.test.ts`, run with `bun test`; `tests/` holds only the shared
fixtures. Use ../CONTRIBUTING.md for required checks and ../docs/releasing.md
for release preparation.

- `release.ts` (+ `release/`) - release preparation, snapshot validation and
  bundled app notes; keep its contracts covered by `release.test.ts`.
- Deployment/layout helpers and their tests must preserve live data and follow
  ../docs/repository-layout-transition.md.
- `sync-csp.ts` - generates every Content-Security-Policy copy. The copies, the
  directives and the desktop variant are documented once, in
  ../frontend/AGENTS.md; read that before changing anything here.
- `build-icons.ts` - generates the favicon, desktop icon SVGs and App icon
  colour variants from one drawing of the mark; `--check` runs in the frontend
  prebuild. `--raster` (`bun run icons` from `frontend/`) also rewrites the
  PNG/ICO/ICNS set and needs `rsvg-convert` and ImageMagick 7; CI cannot
  rasterise, so commit those files with their SVGs.
- `check-probe-report.ts` - the gate for the TENNOWORTH_PROBE smoke run
  (ui-smoke.yml) in the real system webview; the assertions are the script.
- `check-public-surface.ts` - the publication gate stated in ../AGENTS.md. It
  reads the git index only, so it needs no install and runs on docs-only changes.
- `check-attribution.ts` - refuses tool co-author trailers, generated-with
  lines and assistant links in a pull request's commit messages and body
  (`attribution-check` in audit.yml). Locally:
  `bun scripts/check-attribution.ts github/develop..HEAD`.
- `check-lxc-backup.sh` - the workstation-side preservation job for the
  observation corpus. It copies the newest `vzdump-lxc-<vmid>-*.tar.zst` and its
  `.log` off the Proxmox node, verifies the copy (`zstd -t`, a `tar --zstd`
  listing, the newest corpus log read back), records a manifest, rotates older
  archives beyond `KEEP`, and refuses to record or delete anything when
  verification fails. Read-only on the node.

## Maintaining the instruction files

The `AGENTS.md` files are tracked project policy with one owner per rule: a
file links to the owner instead of restating it. `check-agent-instructions.ts`
verifies their relative links resolve and that every tracked `*/AGENTS.md` is
reachable from the root router; it reads the git index, so stage a new file
before checking it, and it does not check backticked paths. CI runs it as
`instruction-check`.

Machine-specific detail goes in the gitignored `AGENTS.local.md`, copied from
../AGENTS.local.md.example; each clone, including the Windows build host, keeps
its own. Keep any tool's provider configuration a thin adapter that points at
these files: no project rules in a tool's private memory, no unused provider
directories, and no reliance on automatic discovery. After changing skills in
the maintainer home or a local adapter, run
`bun scripts/check-agent-instructions.ts --local`, which checks skill
frontmatter and links and adapter targets on disk.
