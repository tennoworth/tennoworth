#!/bin/sh
# Pull the CI-built wfm-scrape pipeline binary (scrape-latest release) when it
# changes and install it to /srv/wfm/bin atomically. Run from the
# wfm-scrape-pull.timer. This binary IS production: run-scrape.sh executes it
# for both the scrape and the market.json build. The box never builds Rust
# (the toolchain stays off it, same posture as node/bun for the web bundle).
# Mirrors pull-web.sh.
set -eu

BIN=/srv/wfm/bin/wfm-scrape
STATE=/srv/wfm/scrape-latest.stamp
API=https://api.github.com/repos/tennoworth/tennoworth/releases/tags/scrape-latest
ASSET=https://github.com/tennoworth/tennoworth/releases/download/scrape-latest/wfm-scrape

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

# The release is republished in place on every push; the asset's updated_at is
# the only reliable change signal (the tag and asset name never change). Match
# the exact wfm-scrape asset (not the sibling .sha256 asset) and take its stamp.
stamp=$(curl -fsSL -H 'Accept: application/vnd.github+json' "$API" \
  | grep -A30 '"name": "wfm-scrape",' \
  | grep -m1 '"updated_at"' \
  | grep -oE '"[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9:]+Z"' \
  | tr -d '"')

if [ -f "$STATE" ] && [ "$(cat "$STATE")" = "$stamp" ]; then
  exit 0
fi

curl -fsSL "$ASSET" -o "$TMP/wfm-scrape"
chmod 0755 "$TMP/wfm-scrape"

# A truncated download or an HTML error page saved as a binary must not replace
# a working one. wfm-scrape has no --help handler (it exits non-zero on any
# no-op invocation), so we can't gate on exit status — instead confirm it
# actually runs and prints its usage banner, which proves a runnable ELF for
# this host's glibc.
if ! "$TMP/wfm-scrape" 2>&1 | grep -q 'usage: wfm-scrape'; then
  echo "downloaded wfm-scrape did not run (corrupt / wrong arch?) — keeping current" >&2
  exit 1
fi

# Atomic install: stage next to the target, then rename over it (same-dir mv is
# atomic, so a concurrent run-scrape.sh execs the whole-old or whole-new binary,
# never a half-written file).
mkdir -p /srv/wfm/bin
install -m 0755 "$TMP/wfm-scrape" "$BIN.new"
mv "$BIN.new" "$BIN"

printf '%s' "$stamp" > "$STATE"
echo "installed wfm-scrape ($stamp)"
