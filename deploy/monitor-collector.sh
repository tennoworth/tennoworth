#!/bin/sh
set -eu
# Alert when one opt-in collector (usage or reports) is down or its published
# aggregate has not been recomputed for two hours.
case "${1:-}" in
  usage) port=8082; route=/api/usage/daily ;;
  reports) port=8083; route=/api/reports/prices ;;
  *) echo 'usage: monitor-collector.sh usage|reports' >&2; exit 2 ;;
esac
# Both endpoints answer 503 while a request or the maintenance tick holds the
# store lock. That is contention, not an outage, so retry briefly before the
# unit fails and alerts; the pullers do the same.
get() {
  for attempt in 1 2 3; do
    curl --fail --silent --show-error --max-time 5 "$1" && return 0
    if [ "$attempt" -lt 3 ]; then sleep 2; fi
  done
  return 1
}
get "http://127.0.0.1:$port/health" >/dev/null
# Parsed with sed rather than jq: nothing provisions jq on the box, and without
# it this monitor failed on every tick from the day it was installed.
updated=$(get "http://127.0.0.1:$port$route" \
  | sed -n 's/.*"updated_at"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p')
[ -n "$updated" ]
last=$(date -d "$updated" +%s)
now=$(date +%s)
[ "$last" -le "$now" ] && [ "$((now-last))" -lt 7200 ]
