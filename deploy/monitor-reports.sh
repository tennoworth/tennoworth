#!/bin/sh
set -eu
# Both endpoints answer 503 while a report or the maintenance tick holds the
# store lock. That is contention, not an outage, so retry briefly before the
# unit fails and alerts; the pullers do the same.
get() {
  for attempt in 1 2 3; do
    curl --fail --silent --show-error --max-time 5 "$1" && return 0
    if [ "$attempt" -lt 3 ]; then sleep 2; fi
  done
  return 1
}
get http://127.0.0.1:8083/health >/dev/null
# sed rather than jq: nothing provisions jq on the box (see monitor-usage.sh).
updated=$(get http://127.0.0.1:8083/api/reports/prices \
  | sed -n 's/.*"updated_at"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p')
[ -n "$updated" ]
last=$(date -d "$updated" +%s)
now=$(date +%s)
[ "$last" -le "$now" ] && [ "$((now-last))" -lt 7200 ]
