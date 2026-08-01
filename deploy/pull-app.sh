#!/usr/bin/env bash
# Update the box's repo checkout at /srv/wfm/app.
#
# The other three pullers (pull-web.sh, pull-scrape.sh, pull-packages.sh) cover
# the built bundle, the wfm-scrape binary and the deb/rpm. NOTHING covers the
# checkout itself, so deploy/run-scrape.sh, the systemd units and the Python
# scraper only move when a human moves them — which is how the box sat on a
# phase-3 commit while main was many commits ahead.
#
# It also re-installs the deployed copies. The units do NOT execute the files
# in the checkout — wfm-scrape.service runs /srv/wfm/run-scrape.sh, a copy
# setup-container.sh made once. On 2026-08-01 that copy was from Jul 19 08:56
# and contained no converter-shadow block, while the repo's copy from 14:59 the
# same day did. Nothing reconciled them, so the shadow "ran" for two weeks in a
# file nothing executed and shadow-parity.log stayed zero bytes.
#
# This is not `git pull`, and the difference matters:
#
#   Caddy serves /market.json and /wfstat-catalog.json from prototype/public/
#   (the @livedata matcher), NOT from dist/. Those two files are the LIVE
#   production snapshot and the box's copies are always newer than the repo's —
#   the box scrapes every 2h, the GitHub cron commits daily. A plain pull either
#   refuses because they're modified, or discards them and serves users an old
#   snapshot until the next scrape.
#
# So: stash the live artifacts, fast-forward, put them back.
#
# Refuses to run while a scrape is in flight. Bash reads a script incrementally,
# so replacing run-scrape.sh underneath a running instance can make it resume
# mid-file at a byte offset that is now different code.
set -euo pipefail

APP="${APP:-/srv/wfm/app}"
REMOTE="${REMOTE:-origin}"
BRANCH="${BRANCH:-main}"

# Generated, box-owned, and newer than anything upstream. Preserved verbatim.
LIVE_ARTIFACTS=(
  prototype/public/market.json
  prototype/public/wfstat-catalog.json
  wfm_results.csv
)

cd "$APP"

if systemctl is-active --quiet wfm-scrape.service 2>/dev/null; then
  echo "ABORT: wfm-scrape.service is running. Re-run when it finishes:" >&2
  echo "  systemctl list-timers wfm-scrape.timer" >&2
  exit 1
fi

before=$(git rev-parse --short HEAD)
git fetch --quiet "$REMOTE" "$BRANCH"
target=$(git rev-parse --short "$REMOTE/$BRANCH")

if [ "$(git rev-parse HEAD)" = "$(git rev-parse "$REMOTE/$BRANCH")" ]; then
  echo "already at $before — nothing to pull"
  exit 0
fi

# Snapshot the live artifacts outside the tree, so a failed merge can't take
# them with it.
stash=$(mktemp -d)
for f in "${LIVE_ARTIFACTS[@]}"; do
  [ -f "$f" ] && { mkdir -p "$stash/$(dirname "$f")"; cp -p "$f" "$stash/$f"; }
done
restore() {
  for f in "${LIVE_ARTIFACTS[@]}"; do
    [ -f "$stash/$f" ] && cp -p "$stash/$f" "$f"
  done
  rm -rf "$stash"
}
trap restore EXIT

# Clear the working-tree modifications the merge would trip over. Safe only
# because we just copied them out — restore() puts them back on every exit path.
for f in "${LIVE_ARTIFACTS[@]}"; do
  git ls-files --error-unmatch "$f" >/dev/null 2>&1 && git checkout -- "$f"
done

git merge --ff-only "$REMOTE/$BRANCH"
echo "pulled: $before -> $target"

# Pulling the checkout is NOT enough. The units execute COPIES under /srv/wfm
# (ExecStart=/srv/wfm/run-scrape.sh), installed once by setup-container.sh and
# never refreshed since. The running /srv/wfm/run-scrape.sh was six hours older
# than the repo's and had no converter-shadow block at all — which is why
# shadow-parity.log sat at zero bytes forever while the repo copy looked fine.
# Re-install anything that drifted, or a pull is cosmetic.
for f in run-scrape.sh pull-web.sh pull-scrape.sh pull-packages.sh; do
  src="deploy/$f"
  [ -f "$src" ] || continue
  if ! cmp -s "$src" "/srv/wfm/$f"; then
    install -m 0755 "$src" "/srv/wfm/$f" && echo "  reinstalled /srv/wfm/$f"
  fi
done

# Units need root plus a daemon-reload, so report rather than act — a puller
# that silently restarts systemd units is a different and larger promise.
for u in wfm-scrape wfm-web-pull wfm-scrape-pull wfm-repo-pull; do
  for ext in service timer; do
    src="deploy/$u.$ext"; dst="/etc/systemd/system/$u.$ext"
    [ -f "$src" ] && [ -f "$dst" ] || continue
    cmp -s "$src" "$dst" || echo "  UNIT DRIFT: $dst differs from $src — install it and daemon-reload"
  done
done

# restore() runs here via the trap, before anyone reads the result.
