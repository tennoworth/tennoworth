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
- Protects per-item quantities and the components of a pinned set goal across
  selling recommendations and ducat planning, alongside the global keep-copy
  setting. Existing listings remain unchanged until reviewed.

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

- Plans a **Trade Session** for Fast Cash, Plat per Trade, Clear Inventory, or
  Max Value within a trade budget and optional platinum target. Complete owned
  sets compete with individual parts without allocating their components twice.
- Compares visible buyers' whole-lot quantity coverage and bid value with your
  listing reference. This uses a top-five sample, not the full order book or a
  guarantee of a sale.
- Reviews existing and proposed warframe.market listings together, with quantity,
  trade-allowance, and changed-order checks before submission. Failed or
  interrupted batches are persisted and can be resumed.
- Shows live orders, quantity mismatches, and listings that have fallen behind
  the current top of book; supports repricing, visibility changes, and removal.
- Watches target prices in the background and sends desktop notifications. A
  live order stream supplies the fast path, with a periodic check as fallback.
- Reads confirmed trades from `EE.log` into a local platinum ledger and can
  shrink or close the matching listing after a sale.
- Keeps a persistent notification inbox for completed trades, price watches,
  scan summaries, Baro reminders, relevant events, and daily sell opportunities,
  with category controls and optional desktop popups.
- Offers an opt-in relic reward overlay. A bounded local capture is OCR'd after
  a reward event or the default `Ctrl+Shift+O` shortcut, recognizes English reward names,
  and adds platinum, ducats, owned count, and recognition confidence. It does
  not click or choose a reward.

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

The optional reward overlay works with Borderless Fullscreen or Windowed mode
on both platforms. Windows and X11 use direct window capture. Wayland capture
currently requires Warframe to run through XWayland; it does not use a capture
portal yet. On supported Wayland compositors, a native layer-shell overlay
displays results above the game, with a desktop-window fallback. Settings
reports capture and presentation backends separately, along with OCR readiness.

## First run

**In the browser:** open [tennoworth.app](https://tennoworth.app/) to browse
market prices and public-data tools. No login or installation is needed.

**In the desktop app:**

1. Start Warframe and continue past its login screen.
2. Open TennoWorth and select **Scan inventory**. No warframe.market login is
   needed to scan your account.
3. Explore your inventory and market context, including Set picks, Relics,
   Rivens, Baro, and market-timing views as they apply to your items.
4. Optional: open **Protected selling plan → Connect WFM** to log in or unlock
   warframe.market. This adds current-listing-aware selling quantities, buyer
   comparisons, and listing management. You can then use Trade Session to plan
   a batch and **List on WFM** to review it before submission.

Current listings are needed to verify available selling quantities. While WFM
is disconnected, those quantities remain unavailable; that does not prevent
inventory scanning or public market browsing.

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
  ledger, protected selling plan, notification history, and pending listing
  plans remain in local application storage.
- Scanning and public market browsing do not require a warframe.market login.
  Verified selling availability and buyer comparisons do. The bearer token is
  encrypted at rest
  with AES-256-GCM using a PBKDF2-derived key. Remember-on-device stores the
  derived unlock key, rather than the passphrase, in the operating system keyring.
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
| [`frontend/`](frontend/) | Svelte 5 + Vite frontend with separate hosted, desktop, and reward-overlay shells. |
| [`rust/`](rust/) | Rust workspace containing the Tauri desktop app, inventory/WFM core, shared market math and client code, and the market pipeline. |
| [`frontend/public/market.json`](frontend/public/market.json) | Production-generated snapshot consumed live by the site and refreshed in Git during desktop release preparation as its bundled fallback. |
| [`scripts/`](scripts/) | TypeScript maintenance gates plus the Linux desktop smoke script. |
| [`tests/fixtures/`](tests/fixtures/) | Cross-language parity and pipeline regression fixtures. |
| [`deploy/`](deploy/) | Self-host deployment kit for the site and scheduled market refresh. |
| [`.github/workflows/`](.github/workflows/) | Web, scraper, desktop release, smoke, and audit automation. |

The Rust workspace contains six crates:

| Crate | Role |
|---|---|
| `tennoworth-desktop` | Tauri v2 shell, local SQLite state, tray, notifications, overlay, update flow, and IPC commands. |
| `wfm-core` | Inventory scan/fetch, encrypted WFM session, listings, orders, and recoverable batch plans. |
| `market-math` | Dependency-free market arithmetic. |
| `market-domain` | Native inventory normalization, sell facts, trade/advisor decisions and planners, with generated IPC contracts. |
| `wfm-client` | Shared warframe.market transport primitives and request policy. |
| `wfm-scrape` | Host pipeline that scrapes market data and builds `market.json` plus `wfstat-catalog.json`. |

## Contributing

Start with [CONTRIBUTING.md](CONTRIBUTING.md) for a working local setup,
contribution tiers, checks, and the pull-request workflow. You can work on the
hosted site and a populated desktop preview without Warframe or credentials.

The [architecture guide](docs/architecture.md) explains feature ownership and
allowed dependencies. UI changes follow the [design system](docs/design-system.md)
and its development-only `?styleguide` reference.

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
