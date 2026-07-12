# scripts/ + Python utilities

One-shot Python tooling. Python is the right call for these — don't
rewrite as Rust.

- `wfm_demand.py` (root) — full WFM scrape (~45 min at 3 req/s; cron
  every 2h). Writes `wfm_results.csv` ONLY. Never point its
  `--json-out` at the public market.json — that path omits
  set_to_parts / relic_rewards / vault_status and blanks the Sets,
  Relics, and Vaulted surfaces.
- `scripts/csv_to_market_json.py` — the SOLE generator of
  `prototype/public/market.json` (full shape) AND
  `prototype/public/wfstat-catalog.json` (the browser resolver's
  item catalog — warframestat dropped CORS, so it's baked here).
  Every scrape, local or cron, must finish with this script (~30 s).

Tests live in `tests/`. Run with `pytest tests/`.

---

## Hard rules

### Atomic writes via `os.replace()`
Long-running scrapes write `wfm_results.csv` and `market.json` in
checkpoints. Concurrent readers (the browser app reloading
`market.json`) must never see a half-written file. Write to
`path.tmp`, then `os.replace(path.tmp, path)`. POSIX rename is atomic
on the same FS. This is already in `wfm_demand.py` — preserve it.

### `flush=True` on progress prints
Python's stdout is block-buffered when piped or redirected. Long
loops with periodic `print()` go silent for minutes without it.
Always add `flush=True` (or `print(..., flush=True)`) when the script
is intended to run unattended.

### HTTP timeouts on every WFM request
WFM has occasional Cloudflare hiccups that hang for minutes. Pass an
explicit `timeout=` to `requests.get/post`. The scrape will recover
better.

### Browser-style User-Agent
WFM's Cloudflare layer 1015-rate-limits generic UAs. Use a real
Firefox/Chrome UA string. Already wired in `wfm_demand.py` — match
that pattern in any new script.

### Don't commit WFM auth secrets
`wfm_demand.py` runs unauthenticated against public WFM data. If you
add a script that needs login, NEVER let credentials reach the repo
or workflow logs. Read from env at runtime; assert on missing env
explicitly.
