#!/bin/sh
set -eu
curl --fail --silent --show-error --max-time 5 http://127.0.0.1:8082/health >/dev/null
updated=$(curl --fail --silent --show-error --max-time 5 http://127.0.0.1:8082/api/usage/daily | jq -er '.updated_at')
last=$(date -d "$updated" +%s)
now=$(date +%s)
[ "$last" -le "$now" ] && [ "$((now-last))" -lt 7200 ]
