# scripts/ — one Node tool + the shared fixtures

Python was fully retired 2026-08: the scraper (`wfm_demand.py`), the converter
(`csv_to_market_json.py`) and the parity/diff tooling (`wfm_common.py`,
`semantic_diff.py`) are gone, replaced by the Rust `wfm-scrape` binary. What
remains in this directory is a single Node tool; the shared cross-language
fixtures live in `tests/fixtures/`.

- `sync-csp.mjs` — the ONE source of truth for the Content-Security-Policy
  that ships in three hosted copies (`prototype/index.html` meta,
  `prototype/public/_headers`, `deploy/Caddyfile`) plus a desktop build
  variant. Edit the `DIRECTIVES` array, run `bun run csp` from `prototype/`
  to rewrite all three; `bun run build` fails via its prebuild `--check` if
  any copy drifted. Do NOT hand-edit the three copies.

Tests live in `tests/` (Rust + TS suites; no pytest).
