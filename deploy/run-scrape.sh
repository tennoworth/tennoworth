#!/usr/bin/env bash
# Refresh market.json: full WFM scrape (~55-65 min; one worker per request the
# signed policy allows in flight, so the 500 ms start-to-start pacing is what
# binds, and items under the volume gate skip the orders call) → CSV, then
# rebuild the full-shape snapshot. This is the ONE production scrape entrypoint,
# driven by the self-hosted systemd timer.
#
# `wfm-scrape scrape` produces wfm_results.csv in DATA, and `wfm-scrape build`
# renders market.json, its slim copies and wfstat-catalog.json from it into
# PUBLISH. Neither is the checkout: the box's live data never shares a tree
# with git, so a pull can neither discard it nor be blocked by it.
#
# Environment (all optional):
#   DATA     the sweep's own state: CSV, statistics cache, WFM cooldown and
#            policy copy (default /srv/wfm/data/scrape)
#   PUBLISH  what the site serves (default /srv/wfm/data/public). A local run
#            sets DATA=. PUBLISH=frontend/public from the repository root.
#   SCRAPE_BIN  Rust pipeline binary      (default /srv/wfm/bin/wfm-scrape)
#   STATS_INTERVAL  how long an item's statistics are reused (default 6h);
#               empty fetches every item's statistics every sweep
#   STATS_CACHE   where reused statistics are kept (default stats-cache.json,
#               beside the CSV)
#   HISTORY     1 (default) also refresh history.json from relics.run after the
#               build; 0 skips it for a one-off local scrape
set -euo pipefail

DATA="${DATA:-/srv/wfm/data/scrape}"
PUBLISH="${PUBLISH:-/srv/wfm/data/public}"
CSV=wfm_results.csv
MIN_ROWS=800                 # absolute floor; a healthy scrape keeps ~2.6k
PUBLISH="$(cd "$PUBLISH" && pwd)"
cd "$DATA"

# Release preparation copies market.json and wfstat-catalog.json as one pair.
# Lock the publish directory's inode for the whole scrape so that copy either
# sees the complete previous generation or waits for this one; it can never
# catch the deliberate catalog-first / market-last publication gap below.
exec 9<"$PUBLISH"
flock -n 9 || { echo "ABORT: another scrape or snapshot operation holds the output lock." >&2; exit 1; }

# Retain the row-count guard as an independent check on upstream shape changes.
prior=0
[ -f "$CSV" ] && prior=$(( $(wc -l < "$CSV") - 1 ))

SCRAPE_BIN="${SCRAPE_BIN:-/srv/wfm/bin/wfm-scrape}"
[ -x "$SCRAPE_BIN" ] || { echo "ABORT: $SCRAPE_BIN is missing - the box needs it (scripts/deploy-scrape-host.sh installs it)." >&2; exit 1; }
SCRAPE_ARGS=(--filter "" --exclude "" --min-volume 1 --out "$CSV")
# Daily medians move once a day and the 48-hour figures drift slowly, while
# the order book moves every sweep. A replay of the since-retired observation corpus
# (317 sweeps, 2026-09-16 to 2026-10-13) put six hours at 42% fewer requests
# with under 1% of top-100 candidates hidden past one extra sweep. A missing
# or unreadable cache only costs a full refresh.
STATS_INTERVAL="${STATS_INTERVAL-6h}"
STATS_CACHE="${STATS_CACHE:-stats-cache.json}"
[ -n "$STATS_INTERVAL" ] && SCRAPE_ARGS+=(--stats-cache "$STATS_CACHE" --stats-interval "$STATS_INTERVAL")

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
"$SCRAPE_BIN" build --data-dir "$DATA" --publish-dir "$PUBLISH"
echo "scrape complete: $now rows, $(date -Is)"

# Long price history (relics.run → $PUBLISH/history.json). Production-only:
# the artifact is its own state (only new days are fetched, normally one file
# per day), so it must live where it persists. A failure here must not fail the
# scrape: history is a bonus surface, market.json is not.
if [ "${HISTORY:-1}" = "1" ]; then
  if ! "$SCRAPE_BIN" history --publish-dir "$PUBLISH"; then
    echo "history: update failed (market.json is unaffected); will retry on the next tick." >&2
  fi
fi
