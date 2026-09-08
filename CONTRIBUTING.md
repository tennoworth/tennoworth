# Contributing to TennoWorth

TennoWorth helps Warframe players decide what to sell now. Windows and Linux
are equal product targets. Contributions should make that decision clearer,
more reliable, or easier to maintain.

## Start with the smallest working environment

For documentation, frontend work, and browser previews, you need Git and Bun.
Use the committed `frontend/bun.lock`; do not create another package lock.
Rust is only needed when working on native code or the market pipeline.

Fork the repository if you do not have write access, clone your fork, and add
the project as an upstream remote. Branch from the latest upstream `develop`:

```sh
git clone https://github.com/YOUR-NAME/tennoworth.git
cd tennoworth
git remote add upstream https://github.com/tennoworth/tennoworth.git
git fetch upstream
git switch -c your-change upstream/develop
cd frontend
bun install --frozen-lockfile
bun run dev
```

Open `http://127.0.0.1:5173/` for the hosted site. The repository includes the
public data needed to render it; a scrape, game installation, account, private
build host, or maintainer configuration is not required.

| Development URL | Purpose |
|---|---|
| `/?preview-desktop&sample` | Desktop shell with fictional inventory, orders, watches, trades, and Riven data |
| `/?preview-desktop&sample=empty` | Empty states |
| `/?preview-desktop&sample=error` | Failed responses |
| `/?preview-desktop&sample=loading` | Delayed responses |
| `/?preview-desktop&sample=logged-out` | Authentication-required states |
| `/?styleguide` | Living design reference, both themes, controls, and editable dialog |

Sample account operations are simulated and reset on reload. These development
surfaces are excluded from production. They do not prove native capture,
window placement, memory access, or real account behavior.

## Choose a contribution tier

Tiers describe the consequences of a change, not contributor seniority. A PR
that crosses boundaries takes the checks of the highest affected tier.

| Tier | Typical work | Evidence to include |
|---|---|---|
| 1 — Documentation | Explanations, setup, examples | Check links and execute changed commands where applicable |
| 2 — Presentation | Feature components, accessibility, layout | Frontend checks and browser evidence in both themes at narrow, short, and wide sizes |
| 3 — Domain rules | Pricing, scoring, item resolution | Behavior tests including missing/invalid data; shared Rust/TypeScript fixtures when logic overlaps |
| 4 — I/O and state | Adapters, cache, pipeline, SQLite | Failure/retry tests, freshness and persistence evidence, affected native checks |
| 5 — Native and account operations | Capture, scan, authentication, order mutation, startup | Native Windows and Linux verification; interrupted operations and permission failures; relevant probes |

Read [architecture.md](docs/architecture.md) before crossing module boundaries
and [design-system.md](docs/design-system.md) before changing UI. Keep feature
state with its feature. Put runtime access behind an injected capability. A
new generic helper or dependency should solve a demonstrated shared need.

## Frontend checks

From `frontend/`:

```sh
bun run check
bun run test
bun run knip
bun run build
bun run build:desktop
bunx playwright install --with-deps chromium webkit
bun run test:responsive:all
```

Playwright starts the development server itself unless `RESPONSIVE_BASE_URL`
is set. Its browser dependencies require a supported host; the matching
Playwright container is an alternative on other Linux distributions.
Failure screenshots and traces are in `frontend/test-results/responsive/`.
The committed visual baselines are Linux Chromium/WebKit baselines. Inspect
intentional changes before updating them, then rerun without updating snapshots.

Browser checks use the real styled frontend with a simulated desktop boundary.
Check the actual app for native behavior. Preserve unfinished form edits through
refreshes, resizes, authentication, and error states. Do not hide diagnostics
with `@ts-nocheck` or replace runtime verification with type checking.

## Native desktop development

Install the Rust toolchain with Cargo. Native dependencies differ by platform;
the current build definitions are in [ui-smoke.yml](.github/workflows/ui-smoke.yml)
and [release-desktop.yml](.github/workflows/release-desktop.yml).

On Ubuntu 22.04, install the build and display packages:

```sh
sudo apt-get update
sudo apt-get install -y build-essential pkg-config clang libclang-dev \
  libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev \
  librsvg2-dev libtesseract-dev libleptonica-dev libxcb1-dev \
  libwayland-dev libxkbcommon-dev xvfb xauth dbus-x11
```

On Windows, use the MSVC Rust toolchain, Visual Studio C++ Build Tools with a
Windows SDK, LLVM/libclang, WebView2, and vcpkg. In your build environment set
`VCPKG_ROOT` to your vcpkg checkout, `VCPKG_DEFAULT_TRIPLET` to
`x64-windows-static-md`, and `LIBCLANG_PATH` to LLVM's `bin` directory. Install
`vcpkg install tesseract:x64-windows-static-md`. If PowerShell selects a blocked
Bun script shim, use `bun.cmd`. The
[Windows OCR runbook](docs/ocr-windows-test-runbook.md) describes the native
capture baseline and isolated test installer.

For OCR, download `eng.traineddata` from
`https://github.com/tesseract-ocr/tessdata_fast/raw/4.1.0/eng.traineddata`
into `rust/tennoworth-desktop/resources/tessdata/`. Verify SHA-256:

```text
7d4322bd2a7749724879683fc3912cb542f19906c83bcc1a52132556427170b2
```

The model is ignored by Git. Use `sha256sum` on Linux or `Get-FileHash` on
Windows. Do not commit downloaded binaries.

From the repository root, build the frontend first, then Cargo:

```sh
cd frontend
bun run build:desktop
cd ../rust
cargo build -p tennoworth-desktop
```

Cargo embeds `frontend/dist-desktop`; plain Cargo does not run the frontend
build hook. A test-only placeholder can let a fresh Cargo test compile, but it
is not a usable app. Do not run the frontend and native builds concurrently.
Launch `rust/target/debug/tennoworth-desktop` from the root on Linux, or the
corresponding `.exe` on Windows.

Scanning requires the game to be running. For a locally built Linux binary,
`sudo setcap cap_sys_ptrace=eip rust/target/debug/tennoworth-desktop` grants
memory-read capability; every rebuild replaces the file and removes that
grant. AppImages have different ptrace constraints; see [README.md](README.md#linux).
No capability grant is needed for UI or fixture tests.

From `rust/`, run the native checks:

```sh
cargo check --workspace
cargo test --workspace
cargo clippy --workspace --all-targets
cargo nextest run
cargo shear
cargo audit --deny warnings
cargo test -p tennoworth-desktop real_three_reward_capture_survives_common_display_shapes -- --ignored
```

Install the separate check tools with `cargo install cargo-nextest cargo-shear cargo-audit --locked`. The ordinary
suite uses local fixtures and mock servers; the opt-in live DE endpoint test
is separate. Mock HTTP tests need permission to bind localhost. Diagnose
permission or disk-space failures before changing product code.

On Linux, from the repository root, run `bash scripts/probe-smoke-linux.sh`.
It rebuilds the desktop bundle and binary, opens the real app under Xvfb and an
isolated D-Bus session, checks the report, and prints the evidence directory.
It tests startup and IPC, not live gameplay capture. On either platform, use
the relevant native workflow/runbook for capture changes.

For large local builds, put `CARGO_TARGET_DIR` and `TMPDIR` on a disk with
sufficient space; a small memory-backed `/tmp` can fill during linking.

## Domain and pipeline work without the desktop toolchain

From `rust/`, `cargo test -p market-math -p wfm-client -p wfm-scrape` exercises
these crates without the desktop's GTK/Tesseract dependencies. Pipeline
integration tests build and run the current binary against frozen fixtures.

Keep shared cases in `tests/fixtures/` and have both languages read the same
expected results. Comments saying “keep in sync” do not prevent drift. Test
unknown values, unavailable sources, malformed siblings, and interrupted
publication as well as usable data. A failed fetch must not turn old data into
fresh data or missing data into zero.

Do not regenerate production snapshots for an unrelated PR. `wfm-scrape build`
is the full snapshot/catalog generator; `scrape` alone only produces CSV. Live
scraping has a separate operational cost and is unnecessary for fixture work.

For maintenance/deployment changes, from the root:

```sh
bun scripts/sync-csp.ts --check
bun test scripts/release.test.ts scripts/deploy-layout.test.ts
```

Edit CSP directives in `scripts/sync-csp.ts`, then run `bun run csp` from
`frontend/`; do not hand-edit the generated policies. Root-layout deployment
changes also require the [transition runbook](docs/repository-layout-transition.md).

## Submit a pull request

Open the PR against `develop`. Describe the user-visible problem, resulting
behavior, checks run, and any verification still pending. For visual changes,
include both themes and relevant sizes. For risky boundaries, explain recovery
and failure behavior. Keep unrelated edits and generated data out of the diff.

Never commit inventory captures, game session secrets, account credentials,
JWTs, private keys, local databases, or diagnostic logs containing personal
information. Use fictional or redacted fixtures. See [SECURITY.md](SECURITY.md)
for reporting security issues and the application's trust boundaries.

Preserve public IPC names, stored formats, and snapshot schemas during a
structural move. Intentional format changes need their own migration and
review. Keep secrets in Rust; the webview must never receive the WFM JWT.
Order mutations must retain pacing, retry policy, explicit user review, and
persisted interrupted-plan recovery.

Run the complete applicable checks before pushing. Required repository checks
still apply when a local environment cannot perform a platform check; state
the gap rather than treating another platform as equivalent. Ordinary feature
PRs do not bump desktop versions. Maintainers handle production promotion and
release tags under [releasing.md](docs/releasing.md).
