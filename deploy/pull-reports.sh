#!/bin/sh
set -eu
# Checksum verification precedes replacement; failed health checks restore the previous binary.
exec 9>/srv/wfm/.reports-pull.lock
flock -n 9 || exit 0
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT HUP INT TERM
base=https://github.com/tennoworth/tennoworth/releases/download/reports-latest
curl --fail --silent --show-error --location "$base/tennoworth-reports" -o "$stage/tennoworth-reports"
curl --fail --silent --show-error --location "$base/tennoworth-reports.sha256" -o "$stage/tennoworth-reports.sha256"
(cd "$stage" && sha256sum --check tennoworth-reports.sha256)
bin=/srv/wfm/bin/tennoworth-reports
mkdir -p /srv/wfm/bin
if cmp -s "$stage/tennoworth-reports" "$bin"; then exit 0; fi
if [ -f "$bin" ]; then cp "$bin" "$bin.previous"; fi
install -m 0755 "$stage/tennoworth-reports" "$bin.new"
mv "$bin.new" "$bin"
# /health answers 503 while a report or the maintenance tick holds the store
# lock, so a single probe can roll back a healthy upgrade. Give it a few tries.
healthy() {
  for attempt in 1 2 3 4 5; do
    sleep 2
    curl --fail --silent --max-time 5 http://127.0.0.1:8083/health >/dev/null && return 0
  done
  return 1
}
if ! (systemctl restart tennoworth-reports.service && healthy); then
  if [ -f "$bin.previous" ]; then mv "$bin.previous" "$bin"; systemctl restart tennoworth-reports.service; fi
  echo 'Reports collector health check failed' >&2
  exit 1
fi
