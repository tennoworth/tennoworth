# TennoWorth

**A Windows and Linux Warframe inventory dashboard built around one decision: what is worth selling now?**

TennoWorth joins the inventory for your current Warframe session with
warframe.market prices, Digital Extremes drop tables, vault rotations, usage
history, and world-state data. It then turns that data into sell priorities,
set-completion plays, relic expected value, Riven context, and listing health.

- **Use the market browser:** [tennoworth.app](https://tennoworth.app) - search,
  prices, volume, trends, vault status, Baro, and market context in a static web
  app. No account or install.
- **Use your own inventory:** install the desktop app from the
  [latest release](https://github.com/tennoworth/tennoworth/releases/latest).
  The inventory scan and account-specific tools are desktop-only.

> [!WARNING]
> TennoWorth reads the running game's memory. It never writes to the game,
> injects code, or automates gameplay, but Digital Extremes has not formally
> approved this category of third-party tool. Ban safety cannot be guaranteed.
> Read [Security and trust](#security-and-trust) before using the scan.

## What it does

### Decide what to sell

- Ranks owned items with a prioritization score based on price, likely
  sell-through, and a bounded DE usage signal. The displayed platinum total is
  still the ordinary, unweighted value of the sellable stack.
- Separates low asks, top buys, 48-hour volume, 7- and 90-day movement, vault
  state, ducat value, owned count, and reserved copies instead of collapsing
  them into one unexplained number.
- Provides focused views for spare mods and Arcanes, ducat fodder, movers,
  sets, hold/sell timing, and vaulted or soon-to-vault items.
- Compares scans so newly acquired and recently removed sellables are visible.

### Find the better play

- **Set picks:** identifies complete and nearly complete Prime sets, prices the
  missing parts, compares the assembled set with its individual parts, and can
  compare building a missing component with buying it.
- **Relic planner:** ranks owned relics by expected platinum per solo crack,
  compares intact sale value, and shows the value added by each refinement.
- **Rivens:** shows scanned stats, rank, rerolls, current disposition and its
  movement, DE's weekly price band, live auction comparables, and offer math.
- **Market timing:** combines Prime release/vault/Resurgence history, annual
  usage changes, current events, Baro's schedule and inventory, and trader
  rotations. Advice remains advice; TennoWorth does not trade automatically.

### Manage the sale

- Reviews and edits a batch before creating warframe.market listings. Failed or
  interrupted batches are persisted and can be resumed.
- Shows live orders, quantity mismatches, and listings that have fallen behind
  the current top of book; supports repricing, visibility changes, and removal.
- Watches target prices in the background and sends desktop notifications. A
  live order stream supplies the fast path, with a periodic check as fallback.
- Reads confirmed trades from `EE.log` into a local platinum ledger and can
  shrink or close the matching listing after a sale.
- Offers an opt-in relic reward overlay. A bounded local capture is OCR'd after
  a reward event or shortcut and annotated with platinum, ducats, owned count,
  and recognition confidence. It does not click or choose a reward.

The hosted site exposes the public-data tools and a guided product preview. It
cannot scan inventory, sign in to warframe.market, create listings, watch
prices in the background, or read `EE.log`.

## Install

### Windows

Download `TennoWorth_<version>_x64-setup.exe` from the
[latest GitHub release](https://github.com/tennoworth/tennoworth/releases/latest).
The installer is not code-signed, so Windows SmartScreen may require
**More info → Run anyway** on first launch.

### Linux

Linux is distributed as one self-updating AppImage:

```bash
curl -LO https://github.com/tennoworth/tennoworth/releases/latest/download/TennoWorth-x86_64.AppImage
curl -LO https://github.com/tennoworth/tennoworth/releases/latest/download/TennoWorth-x86_64.AppImage.sha256
sha256sum -c TennoWorth-x86_64.AppImage.sha256
chmod +x TennoWorth-x86_64.AppImage
./TennoWorth-x86_64.AppImage
```

Linux kernels commonly restrict reading another process even when both belong
to the same user. If the app reports a ptrace permission error, it will include
the detected policy and an actionable command. For a temporary, reboot-scoped
change:

```bash
sudo sysctl kernel.yama.ptrace_scope=0
```

That relaxes same-user ptrace protection system-wide until reboot; do not apply
it without understanding the trade-off. File capabilities are appropriate for
a locally built binary, but do not work on the AppImage's `nosuid` mount.

The optional reward overlay works with Borderless Fullscreen or Windowed mode.
Windows and X11 use direct window capture. Wayland currently requires Warframe
to run through XWayland; the overlay does not use a capture portal yet. Settings
reports the active capture backend and whether OCR is ready.

## First run

1. Start Warframe and continue past its login screen.
2. Open TennoWorth and select **Scan inventory**.
3. Review the ranked Sell view, then explore Set picks, Relics, Rivens, Baro,
   routines, and market-timing views as they apply to the scanned inventory.
4. Optional: use **List on WFM**. TennoWorth asks for warframe.market
   credentials only when an authenticated feature needs them.

Closing the main window hides the desktop app to the system tray so price
watches and trade detection can continue. Quit it from the tray to stop the
process.

## Security and trust

```text
Warframe process ── read-only memory scan ──► desktop app ──► DE inventory API
Warframe EE.log  ── read-only tail ─────────►      │
reward screen    ── opt-in local capture ───►      │
                                                   ├── local SQLite state
warframe.market ── public market data ───────►      ├── sell/relic/set decisions
DE public data  ── drop tables + world state ►      └── optional WFM orders
                         │
                         └──► static market snapshot ──► tennoworth.app
```

- TennoWorth has no user-account backend, telemetry collector, or inventory
  upload service. The hosted site is static and never receives inventory.
- The desktop scan extracts the session values needed to request inventory
  from Digital Extremes. The resulting inventory, snapshots, settings, watches,
  ledger, and pending listing plans remain in local application storage.
- A warframe.market login is optional. Its bearer token is encrypted at rest
  with AES-256-GCM using a PBKDF2-derived key. Remember-on-device stores the
  derived unlock key-not the passphrase-in the operating system keyring.
- Reward captures stay in memory unless the user explicitly enables local
  diagnostics, which writes recent captures under the app cache directory.
  Nothing is uploaded automatically. Optional live overlay pricing sends
  resolved item identifiers to warframe.market, not captured frames.
- Market and Digital Extremes data is fetched by the project pipeline and
  published as static JSON; visitors do not scrape upstream services.

The complete threat model, release-checksum instructions, cryptographic
details, and explicit non-promises live in [`SECURITY.md`](SECURITY.md).

## How the repository is organized

| Path | Purpose |
|---|---|
| [`prototype/`](prototype/) | Svelte 5 + Vite SPA used by both the hosted informational site and the Tauri webview. |
| [`companion/`](companion/) | Rust workspace containing the Tauri desktop app, inventory/WFM core, shared market math and client code, and the market pipeline. |
| [`prototype/public/market.json`](prototype/public/market.json) | Production-generated snapshot consumed live by the site and refreshed in Git during desktop release preparation as its bundled fallback. |
| [`scripts/`](scripts/) | TypeScript maintenance gates plus the Linux desktop smoke script. |
| [`tests/fixtures/`](tests/fixtures/) | Cross-language parity and pipeline regression fixtures. |
| [`deploy/`](deploy/) | Self-host deployment kit for the site and scheduled market refresh. |
| [`.github/workflows/`](.github/workflows/) | Web, scraper, desktop release, smoke, and audit automation. |

The Rust workspace contains five crates:

| Crate | Role |
|---|---|
| `tennoworth-desktop` | Tauri v2 shell, local SQLite state, tray, notifications, overlay, update flow, and IPC commands. |
| `wfm-core` | Inventory scan/fetch, encrypted WFM session, listings, orders, and recoverable batch plans. |
| `market-math` | Pure market heuristics shared by the desktop path and parity-tested against the SPA. |
| `wfm-client` | Shared warframe.market transport primitives and request policy. |
| `wfm-scrape` | Host pipeline that scrapes market data and builds `market.json` plus `wfstat-catalog.json`. |

## Development

The web app requires [Bun](https://bun.sh/):

```bash
cd prototype
bun install --frozen-lockfile
bun run dev
```

The development server listens on `http://127.0.0.1:5173`. It runs in hosted
mode, so desktop-only IPC features are intentionally unavailable.

To build and run the desktop app, build the desktop SPA **before** Cargo; Tauri
embeds that directory into the binary:

```bash
cd prototype
bun install --frozen-lockfile
bun run build:desktop

cd ../companion
cargo build -p tennoworth-desktop
```

On Linux, install the Tauri/WebKitGTK and Tesseract development packages for
your distribution first. A locally built binary can receive the narrower
ptrace capability; rebuilding replaces the file and removes it, so repeat this
after every build, then launch the binary directly:

```bash
sudo setcap cap_sys_ptrace=eip target/debug/tennoworth-desktop
./target/debug/tennoworth-desktop
```

On Windows, launch `target/debug/tennoworth-desktop.exe` after the build; no
ptrace capability step is needed.

Useful local checks mirror the repository's CI surfaces:

```bash
cd prototype
bun run test
bun run check
bun run knip

cd ../companion
cargo test
cargo clippy --workspace --all-targets
cargo shear
cargo audit --deny warnings

cd ..
bun scripts/sync-csp.ts --check
bash scripts/probe-smoke-linux.sh
```

The Linux probe needs `xvfb-run`; the desktop Rust build needs the native
WebKitGTK/GTK/AppIndicator/Tesseract toolchain. Windows builds and smoke tests
run natively in CI.

## Branches and releases

`develop` is the integration branch. `main` is production and deploys the web
app; production promotion is fast-forward only. Desktop releases are built
from `main` and tagged `desktop-v<version>`. The site and market/scraper
artifacts use rolling release tags and are not desktop versions.

See [`CHANGELOG.md`](CHANGELOG.md) for desktop release history and
[`docs/releasing.md`](docs/releasing.md) for the release policy and procedure.

Feature-branch Windows OCR installers are tested with the
[`docs/ocr-windows-test-runbook.md`](docs/ocr-windows-test-runbook.md); they
use a separate app identity and do not replace production TennoWorth.

## License

[MIT](LICENSE). TennoWorth is a fan project and is not affiliated with Digital
Extremes or warframe.market.
