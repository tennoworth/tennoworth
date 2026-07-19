# CLAUDE.md — project overview

Cross-platform **Windows + Linux** Warframe inventory + market dashboard
— the no-Overwolf alternative to AlecaFrame. Windows and Linux are equal,
first-class targets (not Linux-first). Inventory is acquired by a local
memory-scan companion — PC-only by nature. Overlaps with browse.wf and
warframe.me on inventory display — must be **measurably better at
"what to sell right now"** to justify existing.

Detailed rules live in per-domain files. Read the one for the area
you're editing **before** you start writing code there:

- [`companion/CLAUDE.md`](companion/CLAUDE.md) — Rust CLI + loopback
  HTTP server. Memory scan, JWT crypto, WFM API map, cross-platform
  gotchas.
- [`prototype/CLAUDE.md`](prototype/CLAUDE.md) — Svelte 5 + Vite
  browser app. Svelte 5 reactivity gotchas, storage choices, CSP.
- [`scripts/CLAUDE.md`](scripts/CLAUDE.md) — Python utilities and
  scrapers. Atomic writes, flush rules, UA requirements.

---

## What lives where

```
companion/       Rust binary — fetch / login / serve subcommands
prototype/       Svelte 5 + Vite app, deployed as static
prototype/public/market.json    central artifact: the WFM snapshot
scripts/         one-shot Python utilities
wfm_demand.py    main WFM scraper (root, run on cron)
deploy/          self-host kit: Caddyfile, setup script, scrape + web-pull systemd units for the production LXC
tests/           pytest tests for the Python side
.github/workflows/  refresh-market (cron), release-companion (tag), audit
.claude/         Claude Code config — agents, commands, hooks
SECURITY.md      threat model + what we do and don't commit to
```

## Components at a glance

```
┌─ Warframe game ──────────────────────────────────┐
│   /proc/<pid>/mem  or  ReadProcessMemory         │
└────────────────────────┬─────────────────────────┘
                         │ scrape accountId+nonce
                         ▼
            ┌──────── companion CLI ────────┐
            │  fetch  → inventory.json      │
            │  login  → ~/.config/wfminv/   │
            │           wfm-jwt.enc (AES)   │
            │  serve  → 127.0.0.1:RAND      │
            └──────┬────────────┬───────────┘
                   │            │  ↑   X-Session-Token
                   │            │  │   plan / pending / orders
                   ▼            ▼
        inventory.json    ┌── browser app (prototype/) ──┐
        (drop into UI)   ─┤  joins inv × market.json      │
                          │  no backend, no accounts      │
                          └───────────────────────────────┘
                                       ▲
                                       │ GET market.json
                                       │ (refreshed on the box)
                            ┌──────────┴─────────────────────────────┐
                            │  wfm_demand.py                          │
                            │  (systemd timer on the box, 2h;         │
                            │   GH cron refreshes repo copy)          │
                            └─────────────────────────────────────────┘
```

---

## Cross-cutting hygiene rules (apply everywhere)

- **No comments that restate the code.** Comments explain *why* — the
  non-obvious constraint, the past bug they prevent. If removing a
  comment wouldn't confuse a reader, delete it.
- **No backwards-compat shims** for code that hasn't shipped yet.
  Renaming a state field? Bump the storage-key version and move on.
- **Edit existing files** in preference to creating new ones.
- **Match the scope of the request.** Don't refactor surrounding code
  while fixing a bug. Don't add features the task didn't ask for.
- **Verify in the actual runtime.** Browser changes → drive the dev
  server or Playwright. Companion changes → run it. Type-checks and
  test suites verify code correctness, not feature correctness.

## AI-written code — failure modes to catch in your own output

1. **Architectural misalignment** — does the new code follow patterns
   already in the repo, or invent a parallel approach?
2. **Happy-path bias** — error paths are ~2× less likely to be
   correct in AI-written code. Walk the failure modes explicitly:
   empty input, network error, malformed JSON, missing key,
   permission denied.
3. **Tests that pin implementation** — do they exercise the public
   contract or hard-code the current internals? The second is
   worthless.
4. **Quietly broken edge cases** — Maps with no entries, dates near
   year boundaries, sudo vs. non-sudo, empty filter strings.
5. **Verification before claiming done.** For UI changes, drive the
   browser and look at the result. "Build succeeded" ≠ "feature works."

---

## Quick reference

```fish
# Dev server (browser app)
cd prototype && bun install && bun run dev   # http://127.0.0.1:5173

# Rebuild static market.json from the existing CSV (~10 s).
# csv_to_market_json.py is the ONLY generator that produces the full
# shape (set_to_parts / relic_rewards / vault_status). Always finish a
# scrape with it — never point wfm_demand.py --json-out at the public
# market.json, it omits those keys and blanks the Sets/Relics/Vaulted
# surfaces.
python3 scripts/csv_to_market_json.py

# Full WFM scrape (~45 min, 3 req/s) → CSV only, then rebuild the snapshot.
python3 wfm_demand.py --filter "" --exclude "" --min-volume 1 \
  --out wfm_results.csv
python3 scripts/csv_to_market_json.py

# Companion subcommands (all in the same binary). Grant ptrace once so
# fetch needs no sudo — re-run after every `cargo build --release`, which
# wipes the capability:
sudo setcap cap_sys_ptrace=eip companion/target/release/wfm-fetch-inventory
companion/target/release/wfm-fetch-inventory               # default = fetch inventory.json
companion/target/release/wfm-fetch-inventory login         # interactive WFM signin
companion/target/release/wfm-fetch-inventory serve         # loopback HTTP server

# Test sweeps
cd prototype && bun run test
pytest tests/
cd companion && cargo test

# Companion rebuild
cd companion && cargo build --release
```

---

## Open items (as of latest session)

- **Container-110 outbound firewall lockdown.** Drafted, deferred —
  restrict the production LXC's egress to just WFM + the GitHub release
  assets it pulls.
- **Language-consolidation phases 1-3** (see the Rust consolidation
  plan): (1) move the GH cron from 2h → daily now the box owns the live
  refresh; (2) land the `wfm-scrape` converter port; (3) port the
  scraper itself.
- **Verify the `serve` late-JWT-unlock listing path end-to-end** with a
  real WFM login — the 401/503 `needs_login` branches on the first
  listing request are still unverified against a live companion.
