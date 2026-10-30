#!/bin/sh
set -eu
# Install the newest verified build of one opt-in collector (usage or reports).
# Checksum verification precedes replacement; failed health checks restore the previous binary.
case "${1:-}" in
  usage) port=8082; label=Usage ;;
  reports) port=8083; label=Reports ;;
  *) echo 'usage: pull-collector.sh usage|reports' >&2; exit 2 ;;
esac
name=$1
exec 9>"/srv/wfm/.$name-pull.lock"
flock -n 9 || exit 0
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT HUP INT TERM
binary="tennoworth-$name"
base="https://github.com/tennoworth/tennoworth/releases/download/$name-latest"
curl --fail --silent --show-error --location "$base/$binary" -o "$stage/$binary"
curl --fail --silent --show-error --location "$base/$binary.sha256" -o "$stage/$binary.sha256"
(cd "$stage" && sha256sum --check "$binary.sha256")
bin="/srv/wfm/bin/$binary"
mkdir -p /srv/wfm/bin
if cmp -s "$stage/$binary" "$bin"; then exit 0; fi
if [ -f "$bin" ]; then cp "$bin" "$bin.previous"; fi
install -m 0755 "$stage/$binary" "$bin.new"
mv "$bin.new" "$bin"
# /health answers 503 while a request or the maintenance tick holds the store
# lock, so a single probe can roll back a healthy upgrade. Give it a few tries.
healthy() {
  for attempt in 1 2 3 4 5; do
    sleep 2
    curl --fail --silent --max-time 5 "http://127.0.0.1:$port/health" >/dev/null && return 0
  done
  return 1
}
unit="tennoworth-collector@$name.service"
if ! (systemctl restart "$unit" && healthy); then
  if [ -f "$bin.previous" ]; then mv "$bin.previous" "$bin"; systemctl restart "$unit"; fi
  echo "$label collector health check failed" >&2
  exit 1
fi
