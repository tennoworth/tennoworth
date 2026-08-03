# companion/ — Rust workspace

The Rust side of TennoWorth. The product is the **desktop app**
(`tennoworth-desktop`, a Tauri v2 shell) driving `wfm-core` over IPC; the
host pipeline crate `wfm-scrape` feeds the market snapshot. The standalone
CLI companion (`wfm-fetch-inventory` with `fetch`/`login`/`serve`) was
removed on 2026-08-02 — the desktop app replaced it.

## Crates

| Crate | What it is |
|---|---|
| `tennoworth-desktop` | Tauri v2 shell — the app users install. Same-origin webview, `wfm-core` over IPC. No HTTP server, no session token. |
| `wfm-core` | The reusable core: process detection + memory scan, DE inventory fetch, WFM auth + encrypted-JWT storage, listing/order service, pending-plan persistence, dormant DeepSeek assistant relay. **No interactive terminal I/O** — the desktop shell drives it over IPC. |
| `market-math` | Pure market-data heuristics (ported from the retired Python scraper). No I/O, no deps. |
| `wfm-scrape` | Host-only pipeline binary: `scrape` runs the WFM scrape to CSV, `build` renders `market.json` + `wfstat-catalog.json`. The only pipeline — Python was retired 2026-08. |
| `wfm-client` | Shared WFM transport primitives (UA, Cloudflare headers, envelope unwrap, retry backoff). |

Build: `cargo build --release`. Tests: `cargo test`.

The Linux binary needs `cap_sys_ptrace` for the inventory scan (grant once;
re-run after every `cargo build --release`, which wipes the capability):

```bash
sudo setcap cap_sys_ptrace=eip companion/target/release/tennoworth-desktop
```

## Ban risk

The desktop app only ever *reads* memory — it never writes to the game, never
injects code, and doesn't interact with anti-cheat. **We cannot promise this
is ban-safe.** Sainan's
[warframe-api-helper](https://github.com/Sainan/warframe-api-helper) has used
the same read-only approach for years with no documented bans, and AlecaFrame
does the equivalent via Overwolf — but DE has never formally blessed this
category of tool. **Use at your own risk; there is no warranty.**

## License

MIT.
