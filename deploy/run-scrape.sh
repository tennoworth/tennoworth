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

# Phase 2b converter shadow — stash the prior BEFORE csv_to_market_json.py
# overwrites it. The Rust shadow (end of this script) must reconcile against the
# SAME prior snapshot the Python converter consumes, but by the time it runs the
# promote has already replaced prototype/public/market.json with the fresh
# output — so capture the prior here. Gated on the shadow binary so GitHub
# Actions runners (which lack it) do nothing; a stash failure never aborts the
# scrape (set -e), the shadow just skips.
SHADOW_BIN=/srv/wfm/bin/wfm-scrape
SHADOW_PRIOR=
if [ -x "$SHADOW_BIN" ] && [ -f prototype/public/market.json ]; then
  SHADOW_PRIOR=$(mktemp 2>/dev/null) && cp prototype/public/market.json "$SHADOW_PRIOR" 2>/dev/null || SHADOW_PRIOR=
fi

"$PYTHON" scripts/csv_to_market_json.py
echo "scrape complete: $now rows, $(date -Is)"

# Phase 2b converter shadow — run the Rust converter (companion/wfm-scrape
# `build`) against the SAME wfm_results.csv and the SAME prior this scrape used,
# then journal semantic parity vs the just-promoted market.json. This NEVER
# influences production: it writes only into a throwaway dir, and the whole
# block is `|| true` so a shadow crash (or missing/broken binary) can't fail the
# systemd service. Runs only when the binary is present — natural scoping to the
# box (GH runners don't have it).
#
# The two converters fetch upstreams (warframestat / relics / vault / baro) at
# slightly different wall-clock instants, so a surface that changed between the
# two fetches can show a BENIGN transient diff. The cutover gate is zero
# STRUCTURAL / persistent diffs across the observation window — not one clean run.
if [ -x "$SHADOW_BIN" ] && [ -n "$SHADOW_PRIOR" ]; then
  {
    LOG=/srv/wfm/shadow-parity.log
    ts=$(date -Is)
    scratch=$(mktemp -d)

    # find_root() in wfm-scrape anchors on the nearest ancestor holding both
    # prototype/public/ and wfm_results.csv, then reads its prior from — and
    # writes its market.json to — prototype/public/ under that root. Seed the
    # scratch tree so the whole run stays inside it.
    mkdir -p "$scratch/prototype/public"
    cp "$CSV" "$scratch/wfm_results.csv"
    cp "$SHADOW_PRIOR" "$scratch/prototype/public/market.json"
    shadow_out="$scratch/prototype/public/market.json"

    ( cd "$scratch" && "$SHADOW_BIN" build ) > "$scratch/build.log" 2>&1
    rc=$?

    if [ "$rc" -ne 0 ] || [ ! -s "$shadow_out" ]; then
      echo "SHADOW UNAVAILABLE: rust build failed (rc=$rc, $ts)"
      diff_out="rust build failed (rc=$rc):
$(cat "$scratch/build.log" 2>/dev/null)"
    elif diff_out=$("$PYTHON" scripts/semantic_diff.py prototype/public/market.json "$shadow_out" 2>&1); then
      echo "SHADOW PARITY OK ($ts)"
    else
      n=$(printf '%s\n' "$diff_out" | head -n1 | grep -oE '^[0-9]+' || true)
      first=$(printf '%s\n' "$diff_out" | sed -n '2p' | sed 's/^  *//')
      echo "SHADOW PARITY DIFF: ${n:-?} paths — first: ${first:-unknown}"
    fi

    {
      echo "===== shadow parity $ts ====="
      printf '%s\n' "$diff_out"
    } >> "$LOG" 2>/dev/null || true
    # Bound growth: keep the last ~500 lines. Rotate through a private-tmp temp
    # (the service sandbox grants write only to the app dir + this log file, so
    # don't stage a sibling in /srv/wfm), then truncate-rewrite the log in place.
    if [ -f "$LOG" ]; then
      tail -n 500 "$LOG" > "$scratch/parity.trim" 2>/dev/null && cat "$scratch/parity.trim" > "$LOG" 2>/dev/null || true
    fi

    rm -rf "$scratch"
    rm -f "$SHADOW_PRIOR"
  } || true
fi
