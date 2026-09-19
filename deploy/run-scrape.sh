#!/usr/bin/env bash
# Refresh market.json: full WFM scrape (~55-65 min; one worker per request the
# signed policy allows in flight, so the 500 ms start-to-start pacing is what
# binds, and items under the volume gate skip the orders call) → CSV, then
# rebuild the full-shape snapshot. This is the ONE production scrape entrypoint,
# driven by the self-hosted systemd timer.
#
# Rust-only since 2026-08 (Python retired): `wfm-scrape scrape` produces
# wfm_results.csv and `wfm-scrape build` renders BOTH frontend/public/market.json
# and frontend/public/wfstat-catalog.json from it.
#
# Environment (all optional):
#   APP      repo root to run in          (default /srv/wfm/app - the LXC layout)
#   SCRAPE_BIN  Rust pipeline binary      (default /srv/wfm/bin/wfm-scrape)
#   OBSERVATIONS  per-item observation log directory (default
#               /srv/wfm/observations); empty disables the log
#   HISTORY     1 (default) also refresh history.json from relics.run after the
#               build; 0 skips it for a one-off local scrape
set -euo pipefail

APP="${APP:-/srv/wfm/app}"
CSV=wfm_results.csv
MIN_ROWS=800                 # absolute floor; a healthy scrape keeps ~2.6k
cd "$APP"

# Release preparation copies market.json and wfstat-catalog.json as one pair.
# Lock the stable app-directory inode for the whole scrape so that copy either
# sees the complete previous generation or waits for this one; it can never
# catch the deliberate catalog-first / market-last publication gap below.
exec 9<.
flock -n 9 || { echo "ABORT: another scrape or snapshot operation holds the output lock." >&2; exit 1; }

# Retain the row-count guard as an independent check on upstream shape changes.
prior=0
[ -f "$CSV" ] && prior=$(( $(wc -l < "$CSV") - 1 ))

SCRAPE_BIN="${SCRAPE_BIN:-/srv/wfm/bin/wfm-scrape}"
[ -x "$SCRAPE_BIN" ] || { echo "ABORT: $SCRAPE_BIN is missing - the box needs it (scripts/deploy-scrape-host.sh installs it)." >&2; exit 1; }
SCRAPE_ARGS=(--filter "" --exclude "" --min-volume 1 --out "$CSV")
# The observation log is evidence for a later statistics-refresh decision, never
# a publication input: the binary warns and carries on when it cannot be
# written, so a missing directory costs nothing but the log.
OBSERVATIONS="${OBSERVATIONS-/srv/wfm/observations}"
[ -n "$OBSERVATIONS" ] && SCRAPE_ARGS+=(--observations-dir "$OBSERVATIONS")

# Identity of the generation on disk right now. The binary refuses a run that
# keeps nothing, but this is the independent check: a replaced or older binary
# that exits 0 without writing the CSV would otherwise fall through to the floor
# below, which counts the PREVIOUS file, passes, and republishes it.
identity() { stat -c '%i:%s:%Y' "$CSV" 2>/dev/null || echo absent; }
csv_before=$(identity)

echo "scraper: $SCRAPE_BIN scrape"
"$SCRAPE_BIN" scrape "${SCRAPE_ARGS[@]}"

if [ "$(identity)" = "$csv_before" ]; then
  echo "ABORT: scrape exited 0 without replacing $CSV - refusing to rebuild the snapshot from the previous generation." >&2
  exit 1
fi

now=$(( $(wc -l < "$CSV") - 1 ))

if [ "$now" -lt "$MIN_ROWS" ] || { [ "$prior" -gt 0 ] && [ "$now" -lt $(( prior * 3 / 4 )) ]; }; then
  echo "ABORT: scrape kept $now rows (prior $prior) - looks truncated/throttled." >&2
  echo "Keeping the existing market.json; will retry on the next tick." >&2
  exit 1
fi

# `build` writes the catalog BEFORE the snapshot (both atomic tmp+rename), so a
# reader catching the gap sees new-catalog + old-market, never the reverse.
"$SCRAPE_BIN" build
echo "scrape complete: $now rows, $(date -Is)"

# Long price history (relics.run → frontend/public/history.json). Production-only:
# the artifact is its own state (only new days are fetched, normally one file
# per day), so it must live where it persists. A failure here must not fail the
# scrape: history is a bonus surface, market.json is not.
if [ "${HISTORY:-1}" = "1" ]; then
  if ! "$SCRAPE_BIN" history; then
    echo "history: update failed (market.json is unaffected); will retry on the next tick." >&2
  fi
fi
