#!/bin/sh
# Pull the CI-built .deb/.rpm for the newest desktop-v* release, sign them, and
# republish the apt + dnf repositories. Run from wfm-repo-pull.timer.
#
# Mirrors pull-web.sh's shape (stamp file, atomic-ish swap, refuse to publish
# something obviously broken) but differs in one important way: it reads the
# LATEST VERSIONED tag rather than a rolling one. Package repos are historical
# — reprepro keeps the pool, and a package whose checksum changed under a
# version users already installed is a broken repo, so the source must be
# immutable. desktop-latest is republished in place and is therefore unusable
# here.
set -eu

FPR=CC5F8E297E446C5B8D2769AC64093BF63D573CE8
REPO=/srv/wfm/repo
STATE=/srv/wfm/repo-release.stamp
API=https://api.github.com/repos/tennoworth/tennoworth/releases

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

# Newest desktop-v* tag. Releases are returned newest-first; filter for the
# desktop-v* prefix (there are also web-latest / scrape-latest rolling tags).
tag=$(curl -fsSL -H 'Accept: application/vnd.github+json' "$API?per_page=30" \
  | grep -oE '"tag_name": "desktop-v[^"]*"' \
  | head -n1 \
  | sed 's/.*"tag_name": "//; s/"$//')

[ -n "$tag" ] || { echo "no desktop-v* release found" >&2; exit 1; }

if [ -f "$STATE" ] && [ "$(cat "$STATE")" = "$tag" ]; then
  exit 0
fi

echo "publishing $tag"
base="https://github.com/tennoworth/tennoworth/releases/download/$tag"
curl -fsSL "$base/tennoworth-desktop-amd64.deb"         -o "$TMP/pkg.deb"
curl -fsSL "$base/tennoworth-desktop-amd64.deb.sha256"  -o "$TMP/deb.sha256"
curl -fsSL "$base/tennoworth-desktop-x86_64.rpm"        -o "$TMP/pkg.rpm"
curl -fsSL "$base/tennoworth-desktop-x86_64.rpm.sha256" -o "$TMP/rpm.sha256"

# The .sha256 files name the CI-side filenames, so compare hashes directly
# rather than running `sha256sum -c` against a path that no longer matches.
verify() {
  want=$(cut -d' ' -f1 < "$2")
  got=$(sha256sum "$1" | cut -d' ' -f1)
  [ "$want" = "$got" ] || {
    echo "checksum mismatch for $1 — refusing to publish" >&2
    exit 1
  }
}
verify "$TMP/pkg.deb" "$TMP/deb.sha256"
verify "$TMP/pkg.rpm" "$TMP/rpm.sha256"

# ---- apt ----------------------------------------------------------------
# reprepro is idempotent per version: re-adding the same version is a no-op,
# and includedeb signs Release into InRelease using SignWith from conf/
# distributions. It refuses a version it already has, so `|| true` would mask
# a real failure — instead check whether this version is already in the pool.
if reprepro -b "$REPO/apt" list stable | grep -q .; then
  existing=$(reprepro -b "$REPO/apt" --list-format '${version}\n' list stable | head -1)
else
  existing=""
fi
newver=$(dpkg-deb -f "$TMP/pkg.deb" Version)

if [ "$existing" != "$newver" ]; then
  reprepro -b "$REPO/apt" includedeb stable "$TMP/pkg.deb"
else
  echo "apt: $newver already published"
fi

# ---- dnf ----------------------------------------------------------------
# Sign the package itself (dnf's gpgcheck=1) before indexing, so the index
# always describes signed packages and never briefly advertises unsigned ones.
cp "$TMP/pkg.rpm" "$REPO/rpm/"
rpmname=$(basename "$TMP/pkg.rpm")
rpm --addsign "$REPO/rpm/$rpmname" >/dev/null

createrepo_c --update "$REPO/rpm" >/dev/null

# repo_gpgcheck=1 reads repomd.xml.asc. Must be regenerated every time the
# metadata changes, and --yes because a stale signature would otherwise block
# the overwrite and leave dnf verifying an index that no longer exists.
rm -f "$REPO/rpm/repodata/repomd.xml.asc"
gpg --batch --yes --pinentry-mode error --detach-sign --armor \
    -u "$FPR" -o "$REPO/rpm/repodata/repomd.xml.asc" "$REPO/rpm/repodata/repomd.xml"

# ---- publish ------------------------------------------------------------
chown -R wfm:wfm "$REPO"
chmod -R a+rX "$REPO"

printf '%s' "$tag" > "$STATE"
echo "published $tag (deb $newver)"
