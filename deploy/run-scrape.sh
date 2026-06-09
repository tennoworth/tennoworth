#!/usr/bin/env bash
# Refresh market.json: full WFM scrape (~45 min @ 3 req/s) → CSV, then rebuild
# the full-shape snapshot. Run from the repo so the scripts' default paths apply:
# writes wfm_results.csv + prototype/public/market.json in-repo. Both scripts use
# atomic os.replace on the same filesystem, so Caddy never serves a torn file.
#
# NEVER point wfm_demand.py --json-out at the public market.json — that path omits
# set_to_parts / relic_rewards / vault_status and blanks the Sets/Relics/Vaulted
# surfaces. csv_to_market_json.py is the only generator that produces the full shape.
set -euo pipefail

APP=/srv/wfm/app
VENV=/srv/wfm/venv/bin
cd "$APP"

"$VENV/python" wfm_demand.py --filter "" --exclude "" --min-volume 1 --out wfm_results.csv
"$VENV/python" scripts/csv_to_market_json.py
echo "scrape complete: $(date -Is)"
