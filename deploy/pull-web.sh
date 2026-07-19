#!/bin/sh
# Pull the CI-built web bundle (web-latest release) when it changes, swap it
# into place atomically, reload Caddy. Run from the wfm-web-pull.timer — the
# box never builds anything (node/bun stay off it, per README).
set -eu

APP=/srv/wfm/app
DIST="$APP/prototype/dist"
STATE=/srv/wfm/web-latest.stamp
API=https://api.github.com/repos/tennoworth/tennoworth/releases/tags/web-latest
TARBALL=https://github.com/tennoworth/tennoworth/releases/download/web-latest/dist.tgz

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

# The release is republished in place on every push; the asset's updated_at is
# the only reliable change signal (the tag and asset name never change).
stamp=$(curl -fsSL -H 'Accept: application/vnd.github+json' "$API" \
  | python3 -c 'import json,sys; r=json.load(sys.stdin); print([a["updated_at"] for a in r["assets"] if a["name"]=="dist.tgz"][0])')

if [ -f "$STATE" ] && [ "$(cat "$STATE")" = "$stamp" ]; then
  exit 0
fi

curl -fsSL "$TARBALL" -o "$TMP/dist.tgz"
mkdir "$TMP/dist"
tar xzf "$TMP/dist.tgz" -C "$TMP/dist"

# A gutted tarball must not replace a working site.
[ -f "$TMP/dist/index.html" ] && [ -d "$TMP/dist/assets" ] || {
  echo "downloaded bundle is missing index.html or assets/ — keeping current dist" >&2
  exit 1
}

chown -R wfm:wfm "$TMP/dist"
rm -rf "$DIST.old"
[ -d "$DIST" ] && mv "$DIST" "$DIST.old"
mv "$TMP/dist" "$DIST"
rm -rf "$DIST.old"

systemctl reload caddy
printf '%s' "$stamp" > "$STATE"
echo "deployed web-latest ($stamp)"
