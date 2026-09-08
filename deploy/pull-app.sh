#!/usr/bin/env bash
# Update the box's repo checkout at /srv/wfm/app.
#
# The other pullers cover built artifacts, not the checkout itself, so
# deploy/run-scrape.sh and the systemd units only move when
# a human moves them - which is how the box sat on a phase-3 commit while main
# was many commits ahead.
#
# It also re-installs the deployed copies. The units do NOT execute the files
# in the checkout - wfm-scrape.service runs /srv/wfm/run-scrape.sh, a copy
# setup-container.sh made once. On 2026-08-01 that copy was from Jul 19 08:56
# while the repo's copy from 14:59 the same day differed - nothing reconciled
# them, so the box ran the old pipeline script for two weeks while the repo
# copy looked current.
#
# This is not `git pull`, and the difference matters:
#
#   Caddy serves /market.json and /wfstat-catalog.json from prototype/public/
#   (the @livedata matcher), NOT from dist/. Those two files are the LIVE
#   production snapshot and the box's copies are normally newer than the repo's
#   release-time bootstrap pair. A plain pull either refuses because they're
#   modified, or discards them and serves users an old snapshot until the next
#   scrape.
#
# So: stash the live artifacts, fast-forward, put them back.
#
# Refuses to run while a scrape is in flight. Bash reads a script incrementally,
# so replacing run-scrape.sh underneath a running instance can make it resume
# mid-file at a byte offset that is now different code.
set -euo pipefail

APP="${APP:-/srv/wfm/app}"
REMOTE="${REMOTE:-github}"
BRANCH="${BRANCH:-main}"

DEPLOY_ROOT="${DEPLOY_ROOT:-/srv/wfm}"
MIGRATE_LAYOUT=false
case "${1:-}" in
  '') ;;
  --migrate-layout) MIGRATE_LAYOUT=true ;;
  *) echo "usage: pull-app.sh [--migrate-layout]" >&2; exit 2 ;;
esac

layout_at() {
  if git cat-file -e "$1:frontend/package.json" 2>/dev/null; then
    echo frontend
  elif git cat-file -e "$1:prototype/package.json" 2>/dev/null; then
    echo prototype
  else
    echo "ABORT: $1 has no recognized frontend layout." >&2
    return 1
  fi
}

cd "$APP"

# NOT `is-active --quiet`. A Type=oneshot service reports `activating` for the
# WHOLE of its ExecStart and only reaches `active` on completion (and only with
# RemainAfterExit, which wfm-scrape.service does not set) - so `is-active`
# returns non-zero for the entire hour the scrape is actually running, and the
# guard sailed through in precisely the window it exists to block. Observed on
# the box 2026-08-11: `is-active` printed `activating` while a scrape was
# mid-run and the pull proceeded anyway; it was harmless only because
# run-scrape.sh happened not to differ in that commit range.
#
# Test the settled states instead, so any not-settled state counts as busy.
scrape_state=$(systemctl show wfm-scrape.service -p ActiveState --value 2>/dev/null || true)
case "${scrape_state:-unknown}" in
  # `unknown`/empty means systemd could not be queried at all (running this off
  # the box, no such unit) - nothing to collide with, so proceed.
  inactive|failed|unknown) ;;
  *)
    echo "ABORT: wfm-scrape.service is $scrape_state. Re-run when it finishes:" >&2
    echo "  systemctl list-timers wfm-scrape.timer" >&2
    # EX_TEMPFAIL, not 1: a scrape occupies ~60 of every 120 minutes, so
    # wfm-app-pull.timer hits this on about half its ticks. A distinct code lets
    # the unit whitelist "busy, try later" (SuccessExitStatus=75) while every
    # real failure still goes red - and a human running this by hand still sees
    # a non-zero exit.
    exit 75
    ;;
esac

before=$(git rev-parse --short HEAD)
git fetch --quiet "$REMOTE" "$BRANCH"
target=$(git rev-parse --short "$REMOTE/$BRANCH")

if [ "$(git rev-parse HEAD)" = "$(git rev-parse "$REMOTE/$BRANCH")" ]; then
  echo "already at $before - nothing to pull"
  exit 0
fi

source_layout=$(layout_at HEAD)
target_layout=$(layout_at "$REMOTE/$BRANCH")
if [[ "$source_layout" != "$target_layout" && "$MIGRATE_LAYOUT" != true ]]; then
  echo "ABORT: repository layout changes from $source_layout to $target_layout." >&2
  echo "Stop deployment timers and follow docs/repository-layout-transition.md before using --migrate-layout." >&2
  exit 1
fi

# Keep logical artifact names separate from source paths: a fast-forward can
# rename the source tree while the production snapshots are newer than Git.
LIVE_ARTIFACTS=(wfstat-catalog.json history.json market.json)
if [[ "$source_layout" != "$target_layout" ]]; then
  # Keep the scan hotfix endpoint available under the old Caddy root until
  # reload, including any operator override. Ordinary source updates still
  # apply tracked definition changes through Git.
  LIVE_ARTIFACTS=(definitions.json "${LIVE_ARTIFACTS[@]}")
fi
stash=$(mktemp -d)
for name in "${LIVE_ARTIFACTS[@]}"; do
  source="$source_layout/public/$name"
  if [[ -f "$source" ]]; then cp -p "$source" "$stash/$name"; fi
done
if [[ -f wfm_results.csv ]]; then cp -p wfm_results.csv "$stash/wfm_results.csv"; fi
if [[ "$source_layout" != "$target_layout" && -d "$source_layout/dist" ]]; then
  cp -a "$source_layout/dist" "$stash/dist"
fi

restore_file() {
  local source=$1 destination=$2
  cp -p "$source" "$destination.restore" && mv -f "$destination.restore" "$destination"
}

restore_failed() {
  echo "Recovery data retained at $stash; restore failed." >&2
  exit 1
}

restore() {
  local exit_status=$?
  trap - EXIT
  local restore_layout
  restore_layout=$(layout_at HEAD) || {
    echo "Recovery data retained at $stash; cannot determine the checkout layout." >&2
    exit 1
  }
  mkdir -p "$restore_layout/public" || restore_failed
  for name in "${LIVE_ARTIFACTS[@]}"; do
    if [[ -f "$stash/$name" ]]; then
      restore_file "$stash/$name" "$restore_layout/public/$name" || {
        echo "Recovery data retained at $stash; restoring $name failed." >&2
        exit 1
      }
    fi
  done
  if [[ -f "$stash/wfm_results.csv" ]]; then
    restore_file "$stash/wfm_results.csv" wfm_results.csv || restore_failed
  fi
  if [[ -d "$stash/dist" && "$restore_layout" != "$source_layout" ]]; then
    # Preserve the previously served bundle until the matching new bundle is
    # installed. The old directory remains available for the Caddy transition.
    mkdir -p "$restore_layout/dist" || restore_failed
    cp -a "$stash/dist/." "$restore_layout/dist/" || restore_failed
  fi
  if [[ "$restore_layout" != "$source_layout" ]]; then
    # Caddy still serves the old public path until its separately validated
    # configuration is reloaded. Retire this copy only after that switch.
    mkdir -p "$source_layout/public" || restore_failed
    for name in "${LIVE_ARTIFACTS[@]}"; do
      if [[ -f "$stash/$name" ]]; then
        restore_file "$stash/$name" "$source_layout/public/$name" || restore_failed
      fi
    done
  fi
  rm -rf "$stash"
  exit "$exit_status"
}
trap restore EXIT

for name in "${LIVE_ARTIFACTS[@]}"; do
  source="$source_layout/public/$name"
  if git ls-files --error-unmatch "$source" >/dev/null 2>&1; then git checkout -- "$source"; fi
done

# Untracked files that the incoming commits add as TRACKED abort the merge.
# Happens whenever something is hand-placed on the box before it lands in git.
# If the box's copy is byte-identical, drop it and
# let the merge bring it in; if it differs, that is a human decision, not
# something a puller should overwrite.
incoming_untracked=$(git diff --name-only --diff-filter=A HEAD "$REMOTE/$BRANCH" 2>/dev/null || true)
for f in $incoming_untracked; do
  [ -f "$f" ] || continue
  git ls-files --error-unmatch "$f" >/dev/null 2>&1 && continue   # already tracked
  if git show "$REMOTE/$BRANCH:$f" 2>/dev/null | cmp -s - "$f"; then
    rm -f "$f" && echo "  cleared identical untracked $f (merge will restore it)"
  else
    echo "ABORT: untracked $f differs from the incoming version." >&2
    echo "  Compare and remove it yourself; refusing to overwrite." >&2
    exit 1
  fi
done

git merge --ff-only "$REMOTE/$BRANCH"
echo "pulled: $before -> $target"

# Pulling the checkout is NOT enough. The units execute COPIES under /srv/wfm
# (ExecStart=/srv/wfm/run-scrape.sh), installed once by setup-container.sh and
# never refreshed since. The running /srv/wfm/run-scrape.sh was six hours older
# than the repo's - a pull that leaves the copies stale is cosmetic.
# Re-install anything that drifted, or a pull is cosmetic.
#
# pull-app.sh reinstalls ITSELF as well, or a fix to this script is the one
# thing that still needs a human. That makes the install METHOD load-bearing:
# `install` truncates and rewrites in place, and bash reads a script
# incrementally - rewriting the file this process is still executing resumes it
# at a byte offset that is now different code (the same hazard the scrape guard
# above exists for). Stage beside the target and rename over it: rename swaps
# the inode, so the running shell keeps reading the copy it started with.
for f in run-scrape.sh alert.sh pull-app.sh pull-web.sh pull-scrape.sh; do
  src="deploy/$f"
  [ -f "$src" ] || continue
  if ! cmp -s "$src" "$DEPLOY_ROOT/$f" 2>/dev/null; then
    install -m 0755 "$src" "$DEPLOY_ROOT/$f.new" \
      && mv -f "$DEPLOY_ROOT/$f.new" "$DEPLOY_ROOT/$f" \
      && echo "  reinstalled $DEPLOY_ROOT/$f"
  fi
done

# The Caddyfile is deployed the same way the units are - a copy under /etc that
# nothing reconciles. It went stale exactly once and it mattered: the C7
# @livedata entry for definitions.json sat unpublished, so /definitions.json
# was served from the BAKED dist/ copy instead of the live public/ one, and
# editing the file on the box changed nothing. That silently voids the whole
# point of a remote-fix file. Report it for the same reason as the units.
if [ -f deploy/Caddyfile ] && [ -f /etc/caddy/Caddyfile ]; then
  cmp -s deploy/Caddyfile /etc/caddy/Caddyfile \
    || echo "  CADDY DRIFT: /etc/caddy/Caddyfile differs from deploy/Caddyfile - install it, 'caddy validate', then reload"
fi

# Units need root plus a daemon-reload, so report rather than act - a puller
# that silently restarts systemd units is a different and larger promise.
for u in wfm-scrape wfm-app-pull wfm-web-pull wfm-scrape-pull; do
  for ext in service timer; do
    src="deploy/$u.$ext"; dst="/etc/systemd/system/$u.$ext"
    [ -f "$src" ] && [ -f "$dst" ] || continue
    cmp -s "$src" "$dst" || echo "  UNIT DRIFT: $dst differs from $src - install it and daemon-reload"
  done
done

# restore() runs here via the trap, before anyone reads the result.
