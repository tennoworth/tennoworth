#!/bin/sh
set -eu
# Checksum verification precedes replacement; failed health checks restore the previous binary.
exec 9>/srv/wfm/.usage-pull.lock
flock -n 9 || exit 0
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT HUP INT TERM
base=https://github.com/tennoworth/tennoworth/releases/download/usage-latest
curl --fail --silent --show-error --location "$base/tennoworth-usage" -o "$stage/tennoworth-usage"
curl --fail --silent --show-error --location "$base/tennoworth-usage.sha256" -o "$stage/tennoworth-usage.sha256"
(cd "$stage" && sha256sum --check tennoworth-usage.sha256)
bin=/srv/wfm/bin/tennoworth-usage
mkdir -p /srv/wfm/bin
if cmp -s "$stage/tennoworth-usage" "$bin"; then exit 0; fi
if [ -f "$bin" ]; then cp "$bin" "$bin.previous"; fi
install -m 0755 "$stage/tennoworth-usage" "$bin.new"
mv "$bin.new" "$bin"
# /health answers 503 while a check-in or the maintenance tick holds the store
# lock, so a single probe can roll back a healthy upgrade. Give it a few tries.
healthy() {
  for attempt in 1 2 3 4 5; do
    sleep 2
    curl --fail --silent --max-time 5 http://127.0.0.1:8082/health >/dev/null && return 0
  done
  return 1
}
if ! (systemctl restart tennoworth-usage.service && healthy); then
  if [ -f "$bin.previous" ]; then mv "$bin.previous" "$bin"; systemctl restart tennoworth-usage.service; fi
  echo 'Usage collector health check failed' >&2
  exit 1
fi
