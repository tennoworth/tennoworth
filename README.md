# TennoWorth

A cross-platform **Windows + Linux** Warframe inventory + market dashboard.
It answers one question better than anything else: **what should I sell right
now?**

Your inventory is read locally by the desktop app (it memory-scans the running
game — nothing is uploaded, no account login to *us*). It joins your inventory
against a live warframe.market price snapshot and ranks your items by expected
plat, not by a raw average price. Runs on Steam Deck.

The opt-in relic reward overlay recognizes the reward screen locally and puts
platinum, ducats, owned count, confidence, and a conservative best-pick marker
beside each choice. Captured frames stay in memory; only item slugs are used
for optional live warframe.market price requests.

**Try the informational site** at **[tennoworth.app](https://tennoworth.app)** —
a full market browser: prices, 48-hour volume, 7-day trend sparklines, vaulted
items, and the Baro countdown, straight from a snapshot refreshed every 2
hours. No download, no login, no account. Want *your* inventory ranked? That's
the desktop app.

<p align="center">
  <img src="docs/img/market-browser.png" alt="TennoWorth market browser: top movers, vaulted &amp; valuable items, and Baro countdown from a live warframe.market snapshot" width="820">
</p>

Search any tradeable item for its price, volume, and trend — no download, no
login:

<p align="center">
  <img src="docs/img/search.png" alt="Searching &quot;primed&quot; returns every matching item with price, 48-hour volume, and a 7-day sparkline" width="820">
</p>

## How it works

```
Warframe (running)  ──►  desktop app (Tauri)  ──►  "what to sell"
                            │  reads game memory    │
                            │  wfm-core              └──► create/edit
                            ▼                              WFM listings
                       market.json
                       (refreshed on a cron)
```

- **`companion/`** — a Rust workspace. `wfm-core` is the reusable core (scan,
  inventory fetch, WFM auth with encrypted JWT, listings, pending plans);
  `tennoworth-desktop` is the Tauri v2 app that drives it over IPC — the only
  adapter. PC-only by nature — it reads game memory. See
  [`companion/README.md`](companion/README.md).
- **`prototype/`** — the Svelte web app, deployed as the informational site.
  No backend, no accounts, no data leaves your machine.
  `prototype/public/market.json` is the shared price snapshot.

## Desktop app

The desktop app is the product. Install it per platform:

- **Windows** — a single installer, `TennoWorth_<version>_x64-setup.exe`, from
  the [latest release](https://github.com/tennoworth/tennoworth/releases/latest).
  Unsigned, so SmartScreen warns on first run (see
  [`SECURITY.md`](SECURITY.md)). The app updates itself from there.
- **Linux** — a single-file **AppImage**, and nothing else:

  ```bash
  curl -LO https://github.com/tennoworth/tennoworth/releases/latest/download/TennoWorth-x86_64.AppImage
  chmod +x TennoWorth-x86_64.AppImage
  ./TennoWorth-x86_64.AppImage
  ```

  It runs on any distro and self-updates in place through the same signed
  updater feed as Windows. Verify it first if you like — every release
  carries `TennoWorth-x86_64.AppImage.sha256` next to it; see
  [`SECURITY.md`](SECURITY.md).

**Coming from apt, dnf or the AUR?** Those channels are retired. The `.deb`
and `.rpm` packages, the repositories at `tennoworth.app/apt` and `/rpm`, and
the `tennoworth` / `tennoworth-bin` AUR packages are gone from the release.
Nothing you have installed breaks: the repositories stay served and signed,
frozen at their last published version, and simply never offer another update.
Take the AppImage above, then remove the old package and its repo entry.

Why: the AppImage is the only Linux channel that can update itself — the
Tauri updater turns itself on when `$APPIMAGE` is set, and a distro package
can't self-update without fighting the package manager. Four delivery
channels for one platform was three more than could be kept correct.

A note of history on the AppImage: the first one (2026-07) was withdrawn
because it aborted at `Could not create default EGL display:
EGL_BAD_PARAMETER` against rolling-release Mesa — a white window, before the
webview painted. WebKit was never the cause. Root-caused on 2026-08-20 by
A/B on a Mesa 26.2 host: the AppImage bundled ubuntu-22.04's libwayland
client/cursor/egl/server, and the host's Wayland-EGL platform rejects that
2022 client. The build now strips those four libraries and repacks, so the
AppImage uses the host's libwayland (a stable ABI that every desktop Linux
ships) — and the identical bundle runs cleanly. Disabling WebKit's DMABUF
renderer was tried first and measurably did not help; if you see that
mitigation recommended anywhere for this symptom, it is not the fix.

The fixed AppImage has shipped in 0.4.0 and 0.5.0, which is what made
consolidating onto it defensible. If you still hit a white window,
`WEBKIT_DISABLE_COMPOSITING_MODE=1` is the bigger hammer — please open an
issue if you need it, since there is no longer a system-WebKit package to
fall back to.

## Develop

```bash
cd prototype && bun install && bun run dev   # http://127.0.0.1:5173
```

Security posture and threat model: [`SECURITY.md`](SECURITY.md).

### Branching

This repository uses a develop-then-main promotion model — `develop` is the
integration branch (feature branches merge here, CI runs here), `main` is
production (auto-deploys), and promotion is fast-forward only.

## Ban risk

The desktop app only reads: game memory for the inventory scan, the game's own
text log (`EE.log`) for trade/reward detection, and—only after explicit opt-in—a
bounded screenshot of the Warframe window for local reward OCR. It never
writes to the game or injects code.
**We can't promise it's ban-safe.** Other read-only inventory tools have run
for years with no documented bans, but DE has never formally blessed this
category of tool. **Use at your own risk; no warranty.**

For a detailed breakdown of what the desktop app reads and what never leaves
your machine, see the in-app 'Trust & safety' section.

Feature-branch Windows OCR installers are tested with the
[`docs/ocr-windows-test-runbook.md`](docs/ocr-windows-test-runbook.md); they
use a separate app identity and do not replace production TennoWorth.

## License

MIT.
