#!/bin/sh
set -eu
curl --fail --silent --show-error --max-time 5 http://127.0.0.1:8083/health >/dev/null
# sed rather than jq: nothing provisions jq on the box (see monitor-usage.sh).
updated=$(curl --fail --silent --show-error --max-time 5 http://127.0.0.1:8083/api/reports/prices \
  | sed -n 's/.*"updated_at"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p')
[ -n "$updated" ]
last=$(date -d "$updated" +%s)
now=$(date +%s)
[ "$last" -le "$now" ] && [ "$((now-last))" -lt 7200 ]
