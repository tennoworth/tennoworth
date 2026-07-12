#!/usr/bin/env bash
# Refresh market.json: full WFM scrape (~45 min @ 3 req/s) → CSV, then rebuild
# the full-shape snapshot. This is the ONE scrape entrypoint — the self-host
# systemd timer and the GitHub Actions cron both call it, so the truncation
# guard can never drift between the two again (it did once: the guard lived
# here but was missing from CI, and a throttled CI run would have committed a
# gutted snapshot).
#
# NEVER point wfm_demand.py --json-out at the public market.json — that path omits
# set_to_parts / relic_rewards / vault_status. csv_to_market_json.py is the only
# generator that produces the full shape.
#
# Environment (all optional):
#   APP     repo root to run in           (default /srv/wfm/app — the LXC layout)
#   PYTHON  python interpreter to use     (default /srv/wfm/venv/bin/python)
set -euo pipefail

APP="${APP:-/srv/wfm/app}"
PYTHON="${PYTHON:-/srv/wfm/venv/bin/python}"
CSV=wfm_results.csv
MIN_ROWS=800                 # absolute floor; a healthy scrape keeps ~2.6k
cd "$APP"

# Capture the prior row count. wfm_demand.py does NOT fail on a sustained 429 —
# it retries, then skips the throttled item and flushes whatever it got with
# exit 0. So `set -e` won't catch a truncated run, and os.replace gives an
# atomic-but-gutted market.json. Gate the rebuild on row count so a throttled
# scrape can't promote a snapshot missing most items.
prior=0
[ -f "$CSV" ] && prior=$(( $(wc -l < "$CSV") - 1 ))

"$PYTHON" wfm_demand.py --filter "" --exclude "" --min-volume 1 --out "$CSV"
now=$(( $(wc -l < "$CSV") - 1 ))

if [ "$now" -lt "$MIN_ROWS" ] || { [ "$prior" -gt 0 ] && [ "$now" -lt $(( prior * 3 / 4 )) ]; }; then
  echo "ABORT: scrape kept $now rows (prior $prior) — looks truncated/throttled." >&2
  echo "Keeping the existing market.json; will retry on the next tick." >&2
  exit 1
fi

"$PYTHON" scripts/csv_to_market_json.py
echo "scrape complete: $now rows, $(date -Is)"
