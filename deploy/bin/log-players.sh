#!/bin/sh
set -eu
# Steam's concurrent-player count for Warframe (app 230410), one JSONL row per
# tick, one file per UTC month. It is a model input for comparing market volume
# against player activity and is never displayed: Steam's API terms require
# Valve attribution wherever the data is shown. Its history only accumulates
# in wall-clock time, which is why it is collected before it is proven useful.
#
# A failed or unexpected response writes nothing and fails the unit. An absent
# row is a gap in the series; a zero would claim nobody was playing.
# Parsed with sed rather than jq: nothing provisions jq on the box.
dir=${PLAYERS_DIR:-/srv/wfm/players}
url=${PLAYERS_URL:-https://api.steampowered.com/ISteamUserStats/GetNumberOfCurrentPlayers/v1/?appid=230410}
agent='TennoWorth/1 (log-players; +https://tennoworth.app; https://github.com/tennoworth/tennoworth/issues)'

body=$(curl --fail --silent --show-error --max-time 15 --user-agent "$agent" "$url")
# Steam answers {"response":{"player_count":N,"result":1}}; any other result
# code means the count is not a count.
printf '%s' "$body" | grep -q '"result"[[:space:]]*:[[:space:]]*1[^0-9]' || {
  echo "log-players: unexpected response: $body" >&2
  exit 1
}
count=$(printf '%s' "$body" | sed -n 's/.*"player_count"[[:space:]]*:[[:space:]]*\([0-9][0-9]*\).*/\1/p')
[ -n "$count" ] || {
  echo "log-players: no player_count in: $body" >&2
  exit 1
}

mkdir -p "$dir"
printf '{"at":"%s","player_count":%s}\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$count" \
  >> "$dir/players-$(date -u +%Y-%m).jsonl"
