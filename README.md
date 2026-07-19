# TennoWorth

A cross-platform **Windows + Linux** Warframe inventory + market dashboard —
the no-Overwolf alternative to AlecaFrame. It answers one question better than
anything else: **what should I sell right now?**

Your inventory is read locally by a small companion (it memory-scans the
running game — nothing is uploaded, no account login to *us*). The browser app
joins it against a live warframe.market price snapshot and ranks your items by
expected plat, not by a raw average price. Runs on Steam Deck.

You can also try TennoWorth instantly at [tennoworth.app](https://tennoworth.app) —
the landing page is a full market browser with live prices, volume trends,
vaulted items, and a Baro countdown, all from a 2-hourly warframe.market
snapshot. No download required.

## How it works

```
Warframe (running)  ──►  companion  ──►  inventory.json
                                              │
                          market.json  ──►  browser app  ──►  "what to sell"
                          (refreshed                              │
                           on a cron)                   optional: companion
                                                        serve ──► create/edit
                                                                  WFM listings
```

- **`companion/`** — a Rust workspace with multiple crates; the main binary
  `wfm-fetch-inventory` is a thin CLI adapter over the `wfm-core` library
  (scan, inventory fetch, WFM auth with encrypted JWT, listings, pending plans,
  and assistant relay). PC-only by nature (it reads game memory). See
  [`companion/README.md`](companion/README.md).
- **`prototype/`** — the Svelte browser app. No backend, no accounts, no data
  leaves your machine. `prototype/public/market.json` is the shared price
  snapshot.

## Quick start

1. **Get the companion** from the
   [latest release](https://github.com/tennoworth/tennoworth/releases/latest)
   (binaries + `SHA256SUMS`), or build it from source:
   ```bash
   cd companion && cargo build --release
   # binary: companion/target/release/wfm-fetch-inventory
   ```
   (The website's `install.sh` / `install.ps1` one-liners do the download +
   checksum verify for you.)

2. **Fetch your inventory** (Warframe running, past the login screen):
   ```bash
   # Linux: grant ptrace once (re-run after every rebuild — the cap is wiped)
   sudo setcap cap_sys_ptrace=eip ./wfm-fetch-inventory
   ./wfm-fetch-inventory                 # → ./inventory.json (where you run it)
   ```
   On Windows just run `.\wfm-fetch-inventory.exe` from a normal PowerShell.

3. **See what to sell.** Open the app and drop `inventory.json` in. That's the
   whole loop — no `login`/`serve` needed for this.

4. **(Optional) List on warframe.market** straight from the app:
   ```bash
   wfm-fetch-inventory login             # once — interactive, sets a passphrase
   wfm-fetch-inventory serve             # leave running in a terminal
   ```
   Paste the `http://127.0.0.1:<random>?token=…` line it prints into the app's
   Companion tab. That port is random and is **not** the website's `5173`.
   `serve` needs a real terminal for the passphrase prompt — see
   [`companion/README.md`](companion/README.md) for the `--passphrase-stdin`
   escape hatch.

## Desktop app (in development)

A native desktop app (Tauri v2) is being built at
`companion/tennoworth-desktop/` as the future primary interface — same-origin
webview with no browser loopback permissions. See
[`docs/product-plan-2026-07.md`](docs/product-plan-2026-07.md) for the roadmap.

## Develop

```bash
cd prototype && bun install && bun run dev   # http://127.0.0.1:5173
```

Domain-specific notes live in the per-directory `CLAUDE.md` files. Security
posture and threat model: [`SECURITY.md`](SECURITY.md).

## Ban risk

The companion only reads game memory — it never writes, never injects.
**We can't promise it's ban-safe.** Equivalent tools
([warframe-api-helper](https://github.com/Sainan/warframe-api-helper) and
AlecaFrame via Overwolf) have run for years with no documented bans, but DE
has never formally blessed this category of tool. **Use at your own risk; no
warranty.**

For a detailed breakdown of what the companion reads and what never leaves your
machine, see the in-app 'Trust & safety' section.

## License

MIT.
