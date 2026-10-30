#!/usr/bin/env bash
# Update the box's repo checkout at /srv/wfm/app.
#
# The other pullers cover built artifacts, not the checkout itself, so without
# this the checkout only moves when a human moves it - which is how the box sat
# on a phase-3 commit while main was many commits ahead.
#
# It also re-installs the deployed copies of the web, policy, usage and reports pieces.
# The scrape pipeline is not one of them: scripts/deploy-scrape-host.sh is the
# only installer of run-scrape.sh, the binary and the units.
#
# The checkout holds no live data: the pipeline writes under /srv/wfm/data. So
# this is a plain fast-forward, safe while a sweep runs, and a checkout that
# cannot fast-forward is left exactly as it was.
set -euo pipefail

APP="${APP:-/srv/wfm/app}"
REMOTE="${REMOTE:-github}"
BRANCH="${BRANCH:-main}"
DEPLOY_ROOT="${DEPLOY_ROOT:-/srv/wfm}"

cd "$APP"

# An up-to-date checkout still reconciles: a puller older than the commit it
# fetched ran its own install list, so a newly added script lands on the next run.
reconcile_deployed() {
  # Pulling the checkout is NOT enough: the units execute COPIES under /srv/wfm
  # that nothing else refreshes, so a pull that leaves the copies stale is
  # cosmetic. Re-install anything that drifted.
  #
  # Scrape-owned files stay out of this list and out of the drift report below.
  # scripts/deploy-scrape-host.sh is their only writer, and two writers is how the
  # box once ran a weeks-old run-scrape.sh for two weeks while the repo's copy
  # looked current.
  #
  # pull-app.sh reinstalls ITSELF as well, or a fix to this script is the one
  # thing that still needs a human. That makes the install METHOD load-bearing:
  # `install` truncates and rewrites in place, and bash reads a script
  # incrementally - rewriting the file this process is still executing resumes it
  # at a byte offset that is now different code. Stage beside the target and
  # rename over it: rename swaps
  # the inode, so the running shell keeps reading the copy it started with.
  for f in alert.sh pull-app.sh pull-web.sh pull-policy.sh pull-collector.sh monitor-collector.sh log-players.sh; do
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
  for f in Caddyfile Caddyfile-admin; do
    [ -f "deploy/$f" ] && [ -f /etc/caddy/Caddyfile ] || continue
    cmp -s "deploy/$f" "/etc/caddy/$f" \
      || echo "  CADDY DRIFT: /etc/caddy/$f differs from deploy/$f - install it, 'caddy validate', then reload"
  done

  # Units need root plus a daemon-reload, so report rather than act - a puller
  # that silently restarts systemd units is a different and larger promise.
  for u in wfm-app-pull wfm-web-pull wfm-policy-pull wfm-players tennoworth-usage tennoworth-usage-pull tennoworth-usage-monitor tennoworth-reports tennoworth-reports-pull tennoworth-reports-monitor tennoworth-publisher tennoworth-publisher-backup; do
    for ext in service timer; do
      src="deploy/$u.$ext"; dst="/etc/systemd/system/$u.$ext"
      [ -f "$src" ] && [ -f "$dst" ] || continue
      cmp -s "$src" "$dst" || echo "  UNIT DRIFT: $dst differs from $src - install it and daemon-reload"
    done
  done
  for src in deploy/tennoworth-job-*.path; do
    [ -f "$src" ] || continue
    dst="/etc/systemd/system/${src#deploy/}"
    [ -f "$dst" ] || continue
    cmp -s "$src" "$dst" || echo "  UNIT DRIFT: $dst differs from $src - install it and daemon-reload"
  done
}

before=$(git rev-parse --short HEAD)
git fetch --quiet "$REMOTE" "$BRANCH"
target=$(git rev-parse --short "$REMOTE/$BRANCH")

if [ "$(git rev-parse HEAD)" = "$(git rev-parse "$REMOTE/$BRANCH")" ]; then
  echo "already at $before - nothing to pull"
  reconcile_deployed
  exit 0
fi

# Untracked files that the incoming commits add as TRACKED abort the merge.
# Happens whenever something is hand-placed on the box before it lands in git.
# If the box's copy is byte-identical, drop it and let the merge bring it in; if
# it differs, that is a human decision, not something a puller should overwrite.
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

reconcile_deployed
