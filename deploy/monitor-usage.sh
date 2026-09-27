#!/bin/sh
set -eu
curl --fail --silent --show-error --max-time 5 http://127.0.0.1:8082/health >/dev/null
# Parsed with sed rather than jq: nothing provisions jq on the box, and without
# it this monitor failed on every tick from the day it was installed.
updated=$(curl --fail --silent --show-error --max-time 5 http://127.0.0.1:8082/api/usage/daily \
  | sed -n 's/.*"updated_at"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p')
[ -n "$updated" ]
last=$(date -d "$updated" +%s)
now=$(date +%s)
[ "$last" -le "$now" ] && [ "$((now-last))" -lt 7200 ]
