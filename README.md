# TennoWorth

A cross-platform **Windows + Linux** Warframe inventory + market dashboard —
the no-Overwolf alternative to AlecaFrame. It answers one question better than
anything else: **what should I sell right now?**

Your inventory is read locally by a small companion (it memory-scans the
running game — nothing is uploaded, no account login to *us*). The browser app
joins it against a live warframe.market price snapshot and ranks your items by
expected plat, not by a raw average price. Runs on Steam Deck.

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

- **`companion/`** — a ~3 MB Rust CLI (`fetch` / `login` / `serve`). PC-only by
  nature (it reads game memory). See [`companion/README.md`](companion/README.md).
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

## License

MIT.
