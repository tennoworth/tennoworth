#!/bin/sh
# Pull the CI-built web bundle (web-latest release) when it changes, swap it
# into place atomically, reload Caddy. The site itself is served from
# Cloudflare, so this only refreshes the box's rollback copy: it runs when the
# private dashboard asks, or by hand before removing the Cloudflare site route.
# The box never builds anything (node/bun stay off it, per README).
#
# This runs as root and the release is writable by anything that can edit
# release assets, so only a bundle signed by the CI-held collector key, as the
# web bundle, is unpacked. The key lives in the main-only collector-release
# environment. A bundle that does not verify leaves the current dist and the
# stamp alone and fails the unit, so OnFailure alerts and the next run retries.
#
# Rollout: this check ships only after build-web.yml publishes
# dist.tgz.minisig, and web-latest must carry a verifying signature before
# pull-app.sh installs this copy from main.
set -eu

APP=/srv/wfm/app
DIST="$APP/frontend/dist"
STATE=/srv/wfm/web-latest.stamp
API=https://api.github.com/repos/tennoworth/tennoworth/releases/tags/web-latest
TARBALL=https://github.com/tennoworth/tennoworth/releases/download/web-latest/dist.tgz
PUB="${COLLECTOR_SIGNING_PUB:-/etc/tennoworth/collector-signing.pub}"

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

# The release is republished in place on every push; the asset's updated_at is
# the only reliable change signal (the tag and asset name never change). Match
# the exact dist.tgz asset and take its stamp.
stamp=$(curl -fsSL -H 'Accept: application/vnd.github+json' "$API" \
  | grep -A30 '"name": "dist.tgz",' \
  | grep -m1 '"updated_at"' \
  | grep -oE '"[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9:]+Z"' \
  | tr -d '"')

if [ -f "$STATE" ] && [ "$(cat "$STATE")" = "$stamp" ]; then
  exit 0
fi

curl -fsSL "$TARBALL" -o "$TMP/dist.tgz"
curl -fsSL "$TARBALL.minisig" -o "$TMP/dist.tgz.minisig"
[ -r "$PUB" ] || { echo "Refusing the web bundle: $PUB is missing - keeping current dist" >&2; exit 1; }
verified=$(minisign -V -p "$PUB" -m "$TMP/dist.tgz" -x "$TMP/dist.tgz.minisig") || {
  echo "Refusing the web bundle: its signature does not verify - keeping current dist" >&2; exit 1; }
comment=$(printf '%s\n' "$verified" | sed -n 's/^Trusted comment: //p')
case "$comment" in
  "tennoworth-web dist.tgz "*) ;;
  *) echo "Refusing the web bundle: signed for '$comment' - keeping current dist" >&2; exit 1 ;;
esac

mkdir "$TMP/dist"
tar xzf "$TMP/dist.tgz" -C "$TMP/dist"

# A gutted tarball must not replace a working site.
[ -f "$TMP/dist/index.html" ] && [ -d "$TMP/dist/assets" ] || {
  echo "downloaded bundle is missing index.html or assets/ - keeping current dist" >&2
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
