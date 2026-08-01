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
# PHASE 5, FIRST HALF: the SCRAPE step now runs the Rust port
# (`wfm-scrape scrape`) by default; wfm_demand.py remains as the fallback and
# the rollback. The CONVERT step is deliberately NOT cut over — its production
# evidence is the shadow journal below, and that has to be read before the
# Python converter stops being the generator.
#
# ACTIVATION: nothing pulls /srv/wfm/app — pull-web.sh, pull-scrape.sh and
# pull-packages.sh cover the dist, the binary and the packages, but the repo
# checkout is updated by hand. Until someone runs `git -C /srv/wfm/app pull`,
# the box keeps executing the previous version of THIS FILE and stays on
# wfm_demand.py. The binary side needs nothing: build-scrape publishes on every
# main push and wfm-scrape-pull.timer installs it every 30 min, so
# /srv/wfm/bin/wfm-scrape is already current.
#
# After pulling, confirm the switch took: the run logs `scraper: <path> scrape`
# on the Rust path and `scraper: wfm_demand.py` on the Python one.
#
# Environment (all optional):
#   APP      repo root to run in          (default /srv/wfm/app — the LXC layout)
#   PYTHON   python interpreter to use    (default /srv/wfm/venv/bin/python)
#   SCRAPE_BIN  Rust pipeline binary      (default /srv/wfm/bin/wfm-scrape)
#   WFM_SCRAPER auto | rust | python      (default auto)
#     auto   — Rust when the binary is present, else Python with a warning.
#              A pull that hasn't landed yet degrades to the old path instead
#              of failing the timer.
#     rust   — require the Rust scraper; fail loudly if it's missing.
#     python — rollback switch. Set this in the unit and the box is back on
#              wfm_demand.py without a deploy.
set -euo pipefail

APP="${APP:-/srv/wfm/app}"
PYTHON="${PYTHON:-/srv/wfm/venv/bin/python}"
CSV=wfm_results.csv
MIN_ROWS=800                 # absolute floor; a healthy scrape keeps ~2.6k
cd "$APP"

# Capture the prior row count. NEITHER scraper fails on a sustained 429 — both
# retry, then skip the throttled item and flush whatever they got with exit 0
# (the Rust port reproduces that behaviour deliberately; it is parity-gated on
# it). So `set -e` won't catch a truncated run, and the atomic replace gives an
# atomic-but-gutted market.json. Gate the rebuild on row count so a throttled
# scrape can't promote a snapshot missing most items. This guard is the only
# thing standing behind the scraper cutover — keep it ahead of the promote.
prior=0
[ -f "$CSV" ] && prior=$(( $(wc -l < "$CSV") - 1 ))

SCRAPE_BIN="${SCRAPE_BIN:-/srv/wfm/bin/wfm-scrape}"
WFM_SCRAPER="${WFM_SCRAPER:-auto}"
SCRAPE_ARGS=(--filter "" --exclude "" --min-volume 1 --out "$CSV")

case "$WFM_SCRAPER" in
  python)
    echo "scraper: wfm_demand.py (WFM_SCRAPER=python)"
    "$PYTHON" wfm_demand.py "${SCRAPE_ARGS[@]}"
    ;;
  rust)
    [ -x "$SCRAPE_BIN" ] || { echo "ABORT: WFM_SCRAPER=rust but $SCRAPE_BIN is missing." >&2; exit 1; }
    echo "scraper: $SCRAPE_BIN scrape"
    "$SCRAPE_BIN" scrape "${SCRAPE_ARGS[@]}"
    ;;
  auto)
    if [ -x "$SCRAPE_BIN" ]; then
      echo "scraper: $SCRAPE_BIN scrape"
      "$SCRAPE_BIN" scrape "${SCRAPE_ARGS[@]}"
    else
      echo "WARN: $SCRAPE_BIN missing — falling back to wfm_demand.py." >&2
      "$PYTHON" wfm_demand.py "${SCRAPE_ARGS[@]}"
    fi
    ;;
  *)
    echo "ABORT: WFM_SCRAPER must be auto|rust|python, got '$WFM_SCRAPER'." >&2; exit 1
    ;;
esac
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
SHADOW_BIN="$SCRAPE_BIN"
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
