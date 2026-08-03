#!/usr/bin/env bash
# Refresh market.json: full WFM scrape (~45 min @ 3 req/s) → CSV, then rebuild
# the full-shape snapshot. This is the ONE scrape entrypoint — the self-host
# systemd timer and the GitHub Actions cron both call it, so the truncation
# guard can never drift between the two again.
#
# Rust-only since 2026-08 (Python retired): `wfm-scrape scrape` produces
# wfm_results.csv and `wfm-scrape build` renders BOTH prototype/public/market.json
# and prototype/public/wfstat-catalog.json from it.
#
# Environment (all optional):
#   APP      repo root to run in          (default /srv/wfm/app — the LXC layout)
#   SCRAPE_BIN  Rust pipeline binary      (default /srv/wfm/bin/wfm-scrape)
set -euo pipefail

APP="${APP:-/srv/wfm/app}"
CSV=wfm_results.csv
MIN_ROWS=800                 # absolute floor; a healthy scrape keeps ~2.6k
cd "$APP"

# Capture the prior row count. The scraper never fails on a sustained 429 — it
# retries, then skips the throttled item and flushes whatever it got with exit 0.
# So `set -e` won't catch a truncated run, and the atomic replace gives an
# atomic-but-gutted market.json. Gate the rebuild on row count so a throttled
# scrape can't promote a snapshot missing most items.
prior=0
[ -f "$CSV" ] && prior=$(( $(wc -l < "$CSV") - 1 ))

SCRAPE_BIN="${SCRAPE_BIN:-/srv/wfm/bin/wfm-scrape}"
[ -x "$SCRAPE_BIN" ] || { echo "ABORT: $SCRAPE_BIN is missing — the box needs it (wfm-scrape-pull.timer installs it)." >&2; exit 1; }
SCRAPE_ARGS=(--filter "" --exclude "" --min-volume 1 --out "$CSV")

echo "scraper: $SCRAPE_BIN scrape"
"$SCRAPE_BIN" scrape "${SCRAPE_ARGS[@]}"
now=$(( $(wc -l < "$CSV") - 1 ))

if [ "$now" -lt "$MIN_ROWS" ] || { [ "$prior" -gt 0 ] && [ "$now" -lt $(( prior * 3 / 4 )) ]; }; then
  echo "ABORT: scrape kept $now rows (prior $prior) — looks truncated/throttled." >&2
  echo "Keeping the existing market.json; will retry on the next tick." >&2
  exit 1
fi

# `build` writes the catalog BEFORE the snapshot (both atomic tmp+rename), so a
# reader catching the gap sees new-catalog + old-market, never the reverse.
"$SCRAPE_BIN" build
echo "scrape complete: $now rows, $(date -Is)"
