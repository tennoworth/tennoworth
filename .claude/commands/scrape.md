---
description: Run the full WFM scrape (~45 min, 3 req/s). Updates prototype/public/market.json on completion.
---

This is a long-running command. Before launching, confirm with the
user that they actually want a full scrape (not just a CSV→JSON
rebuild, which takes ~10 s).

If they want the fast path:
```fish
python3 scripts/csv_to_market_json.py
```

If they confirm the full scrape, run via `run_in_background: true`.
`wfm_demand.py` writes the CSV only — it does NOT emit the
`set_to_parts` / `relic_rewards` / `vault_status` keys the Sets,
Relics, and Vaulted surfaces need. Never give it `--json-out` to the
public market.json; rebuild the snapshot with `csv_to_market_json.py`
once the scrape finishes.
```fish
python3 wfm_demand.py --filter "" --exclude "" --min-volume 1 \
  --out wfm_results.csv
```

Set a Monitor on the background task so you're notified when it
completes. Don't poll. While it runs, you can do other work; come
back to the result when notified.

When the scrape finishes, rebuild the public snapshot from the fresh
CSV (this is the step that produces the complete market.json):
```fish
python3 scripts/csv_to_market_json.py
```
