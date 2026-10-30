#!/usr/bin/env bash
# Deploy the wfm-scrape pipeline to the host directly, from a reviewed revision.
#
# The scrape is host-only infrastructure - the pipeline binary, the verifier
# built with it to prove its key, one driver script, the host check and their
# units, and nothing a user installs contains any of them - so it does
# not need the GitHub release relay the web bundle and desktop app need. Commits
# still live in the monorepo and are still reviewed; this script installs an
# immutable revision of them, and nothing else.
#
# Order of operations, all of which are load-bearing:
#   1. the tree is clean and the revision is already on the remote,
#   2. the crate's own gates pass (they are the only thing that catches field
#      drift and crash-family regressions in this pipeline),
#   3. the artifact links no glibc newer than the box's,
#   4. the sweep timer is held stopped and any sweep it already started is
#      drained, so one sweep cannot span two releases,
#   5. the release is installed on the box and proven there - checksums, the
#      usage banner, and the revision's own verifier accepting the live policy -
#      while the live paths still run the previous release,
#   6. only then are the live paths replaced, so a rejection leaves what is
#      running exactly as it was,
#   7. /srv/wfm/deployed.json records what ran and what it was checked against,
#   8. the sweep schedule is restored, and only then is the host check armed.
#
# Environment:
#   HOST          ssh target                (default wfm)
#   REVISION      commit to deploy          (default HEAD)
#   HOST_ROOT     deployment root on the box (default /srv/wfm)
#   SSH / SCP     ssh and scp commands  (default "ssh" and "scp")
#   DRY_RUN       1 builds and checks, then prints what it would install
#   TENNOWORTH_WFM_POLICY_PUBLIC_KEY  required; the base64 Minisign public key
set -euo pipefail

HOST="${HOST:-wfm}"
# Overridable because a session that cannot read the system ssh config must pass
# its own. Deliberately unquoted at the call sites so SSH can carry options:
# SSH='ssh -F /home/you/.ssh/config'.
SSH="${SSH:-ssh}"
SCP="${SCP:-scp}"
HOST_ROOT="${HOST_ROOT:-/srv/wfm}"
REVISION="${REVISION:-$(git rev-parse HEAD)}"
DRY_RUN="${DRY_RUN:-0}"
RELEASES="$HOST_ROOT/releases"
STAGING="$HOST_ROOT/staging/$REVISION"
PUBLISHED_POLICY_URL=https://github.com/tennoworth/tennoworth/releases/download/wfm-policy-latest/wfm-policy.json
REMOTE="${REMOTE:-github}"

: "${TENNOWORTH_WFM_POLICY_PUBLIC_KEY:?set TENNOWORTH_WFM_POLICY_PUBLIC_KEY - without it the deployed scraper silently ignores the signed policy}"

say() { printf '%s\n' "$*"; }
die() { printf 'ABORT: %s\n' "$*" >&2; exit 1; }

# ---- 1. reviewed, clean revision -------------------------------------------
[ -z "$(git status --porcelain)" ] || die "the working tree is dirty; deploy a committed revision"
[ "$(git rev-parse HEAD)" = "$REVISION" ] || die "REVISION must be HEAD ($(git rev-parse --short HEAD)) for a reproducible build"
git rev-parse --verify --quiet "$REMOTE/develop" >/dev/null || die "fetch $REMOTE first"
git merge-base --is-ancestor "$REVISION" "$REMOTE/develop" \
  || die "$REVISION is not on $REMOTE/develop - deploy a reviewed revision"
ROOT="$(git rev-parse --show-toplevel)"
say "deploying $(git rev-parse --short "$REVISION") from $(git branch --show-current 2>/dev/null || echo 'a detached HEAD')"

# ---- 2. the pipeline's own gates -------------------------------------------
say "checks: cargo test + clippy -p wfm-scrape"
# The workspace root is rust/, not the repository root - the dry run caught the
# script running cargo where there is no Cargo.toml.
(cd "$ROOT/rust" && cargo test -p wfm-scrape)
(cd "$ROOT/rust" && cargo clippy -p wfm-scrape --all-targets)

# ---- 3. build, then refuse an artifact the box cannot load ------------------
say "build: release, locked, policy key compiled in"
(cd "$ROOT/rust" && TENNOWORTH_WFM_POLICY_PUBLIC_KEY="$TENNOWORTH_WFM_POLICY_PUBLIC_KEY" \
  cargo build --release --locked -p wfm-scrape)
# The policy proof has to come from this revision's own verifier. Whatever
# /srv/wfm/bin/wfm-policy already held was built from some other revision, so its
# success said nothing about the scraper being installed. Both binaries compile
# wfm-client's single option_env! into PUBLIC_KEY, so a verifier built here, in
# the same tree and with the same key, accepts the live policy only when the key
# this revision was built with is the key that signed it.
(cd "$ROOT/rust" && TENNOWORTH_WFM_POLICY_PUBLIC_KEY="$TENNOWORTH_WFM_POLICY_PUBLIC_KEY" \
  cargo build --release --locked -p wfm-client --bin wfm-policy)
# Honor CARGO_TARGET_DIR: CI and sandboxes set it, and the artifact is not under
# rust/target when they do.
TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/rust/target}"
ARTIFACT="$TARGET_DIR/release/tennoworth-pipeline"
VERIFIER_ARTIFACT="$TARGET_DIR/release/wfm-policy"
[ -x "$ARTIFACT" ] || die "no artifact at $ARTIFACT"
[ -x "$VERIFIER_ARTIFACT" ] || die "no verifier at $VERIFIER_ARTIFACT"

# The box is newer than the build host in practice (Debian 13 today), but glibc
# has no forward compatibility: a binary needing a symbol newer than the box's
# fails at start. Compare symbol versions rather than trusting the build host.
BOX_GLIBC="$($SSH "$HOST" "ldd --version | head -1 | grep -oE '[0-9]+\.[0-9]+$'")"
[ -n "$BOX_GLIBC" ] || die "could not read the box's glibc version"
WANTED_GLIBC="$(objdump -T "$ARTIFACT" "$VERIFIER_ARTIFACT" | grep -oE 'GLIBC_[0-9]+\.[0-9]+' | sed 's/GLIBC_//' | sort -V | tail -1)"
[ -n "$WANTED_GLIBC" ] || die "could not read the artifact's required glibc symbols"
newest="$(printf '%s\n%s\n' "$BOX_GLIBC" "$WANTED_GLIBC" | sort -V | tail -1)"
[ "$newest" = "$BOX_GLIBC" ] \
  || die "artifact needs GLIBC_$WANTED_GLIBC but the box has $BOX_GLIBC"
say "glibc: needs <= $WANTED_GLIBC, box has $BOX_GLIBC"

CHECKSUM="$(sha256sum "$ARTIFACT" | cut -d' ' -f1)"
VERIFIER_CHECKSUM="$(sha256sum "$VERIFIER_ARTIFACT" | cut -d' ' -f1)"
say "sha256 $CHECKSUM"

if [ "$DRY_RUN" = 1 ]; then
  say "DRY RUN - would install to $RELEASES/$REVISION and record $HOST_ROOT/deployed.json"
  exit 0
fi

# The pipeline writes only under $HOST_ROOT/data. A box whose live files still
# sit in the checkout would come out of this deploy serving nothing.
$SSH "$HOST" "test -f '$HOST_ROOT/data/public/market.json' && test -d '$HOST_ROOT/data/scrape'" \
  || die "$HOST_ROOT/data/public/market.json is missing - move the live data out of the checkout first"

# ---- 4. hold the schedule, then drain what it already started --------------
# Checking for an idle box before staging does not close the window: the timer
# can fire at any point in the install, and the live files are replaced one at a
# time, so a single sweep could run `scrape` from one release and `build` from
# the next. The timer has to be stopped for the whole operation.

# The schedule must come back on the failure path as well as the success one. A
# deploy that dies with the timer stopped leaves the box with no schedule at all
# and no error anyone sees until the data is stale. Arm the restore before the
# stop: if the SSH session dies after the remote systemctl stopped the timer but
# before this script sees that, a trap armed afterwards would never run. Killing
# only the ssh child does not reliably skip this trap - the parent shell still
# exits through it - but a connection lost after the stop and before the start
# takes effect leaves the timer stopped with nobody left to notice, so that state
# has to be detectable by hand:
#   systemctl list-timers tennoworth-scrape.timer
#   systemctl start tennoworth-scrape.timer
# A SIGKILL or a host power loss runs none of this; the unit stays enabled, so a
# reboot re-arms it through timers.target even then.
TIMER_HELD=1
restore_timer() {
  [ "${TIMER_HELD:-0}" = 1 ] || return 0
  TIMER_HELD=0
  # Return non-zero when the restore does not take effect. In the EXIT trap the
  # status is discarded, but the success path calls this explicitly and a deploy
  # that leaves the box with no schedule must not report success.
  # Until activation installs it, a box from before the unit rename has only
  # the old wfm-scrape.timer, and that is the schedule the hold stopped.
  $SSH "$HOST" "if [ -f /etc/systemd/system/tennoworth-scrape.timer ]; then systemctl start tennoworth-scrape.timer; else systemctl start wfm-scrape.timer; fi" && return 0
  say "WARNING: restore the timer by hand and confirm it: systemctl list-timers tennoworth-scrape.timer; systemctl start tennoworth-scrape.timer"
  return 1
}
trap restore_timer EXIT

$SSH "$HOST" "systemctl stop tennoworth-scrape.timer wfm-scrape.timer >/dev/null 2>&1 || true; ! systemctl is-active --quiet tennoworth-scrape.timer && ! systemctl is-active --quiet wfm-scrape.timer" \
  || die "could not stop tennoworth-scrape.timer; refusing to install under a live schedule"

# Stopping the timer does not stop a sweep a previous elapse already started.
# `systemctl is-active` exits non-zero for inactive and failed, so its exit
# status cannot separate "the box says terminal" from "the query never ran".
# Read the state string and accept only what systemd can report for a oneshot: an
# empty or unrecognised result is a dead connection or a missing systemctl, and
# treating that as idle is how this check fails open.
# The sweep is also asked about under its pre-rename name, wfm-scrape.service,
# which a box not yet deployed past the rename still runs; systemd reports an
# absent unit as inactive.
sweep_state() {
  local states state running=""
  if ! states="$($SSH "$HOST" "systemctl is-active tennoworth-scrape.service wfm-scrape.service" 2>/dev/null)"; then
    : # inactive and failed exit non-zero; the state strings below decide
  fi
  [ "$(printf '%s\n' "$states" | grep -c .)" = 2 ] \
    || die "could not read tennoworth-scrape.service state (got '${states:-nothing}'); refusing to install"
  while IFS= read -r state; do
    case "$state" in
      active|activating|deactivating) running="$state" ;;
      inactive|failed) ;;
      *) die "could not read tennoworth-scrape.service state (got '$state'); refusing to install" ;;
    esac
  done <<<"$states"
  printf '%s\n' "${running:-inactive}"
}

# Wait the sweep out before writing anything. A transitional state means the
# service is still running, and one sample can leave it running through the
# install; only a terminal state lets the deploy continue.
while :; do
  state="$(sweep_state)"
  case "$state" in
    active|activating|deactivating)
      say "a sweep is $state; holding the schedule until it reaches a terminal state"
      sleep 30 ;;
    inactive|failed) break ;;
  esac
done

# ---- 5. stage the whole release -------------------------------------------
say "staging to $STAGING"
$SSH "$HOST" "install -d -m 0755 -o root -g root '$STAGING'"
$SCP -q "$ARTIFACT" "$HOST:$STAGING/tennoworth-pipeline"
$SCP -q "$VERIFIER_ARTIFACT" "$HOST:$STAGING/wfm-policy"
$SCP -q deploy/run-scrape.sh deploy/tennoworth-scrape.service deploy/tennoworth-scrape.timer "$HOST:$STAGING/"
$SCP -q deploy/host-check.sh deploy/tennoworth-host-check.service deploy/tennoworth-host-check.timer "$HOST:$STAGING/"

# ---- 6. install and prove the release before the live paths move -----------
# The live paths keep running the previous release until this one is proven on
# the box. A rejected policy has to leave what is deployed exactly as it was,
# not a half-swapped tree that the restored timer then runs against.
$SSH "$HOST" bash -s <<REMOTE_RELEASE
set -euo pipefail
install -d -m 0755 -o root -g root "$RELEASES/$REVISION"
install -m 0755 "$STAGING/tennoworth-pipeline" "$RELEASES/$REVISION/tennoworth-pipeline"
install -m 0755 "$STAGING/wfm-policy" "$RELEASES/$REVISION/wfm-policy"
install -m 0755 "$STAGING/run-scrape.sh" "$RELEASES/$REVISION/run-scrape.sh"
install -m 0644 "$STAGING/tennoworth-scrape.service" "$RELEASES/$REVISION/tennoworth-scrape.service"
install -m 0644 "$STAGING/tennoworth-scrape.timer" "$RELEASES/$REVISION/tennoworth-scrape.timer"
install -m 0755 "$STAGING/host-check.sh" "$RELEASES/$REVISION/host-check.sh"
install -m 0644 "$STAGING/tennoworth-host-check.service" "$RELEASES/$REVISION/tennoworth-host-check.service"
install -m 0644 "$STAGING/tennoworth-host-check.timer" "$RELEASES/$REVISION/tennoworth-host-check.timer"
REMOTE_RELEASE

SCRAPER="$RELEASES/$REVISION/tennoworth-pipeline"
VERIFIER="$RELEASES/$REVISION/wfm-policy"

$SSH "$HOST" "'$SCRAPER' 2>&1 | grep -q 'usage: tennoworth-pipeline'" \
  || die "the built binary does not run on the box"
[ "$($SSH "$HOST" "sha256sum '$SCRAPER' | cut -d' ' -f1")" = "$CHECKSUM" ] \
  || die "the release binary is not the one that was built"
[ "$($SSH "$HOST" "sha256sum '$VERIFIER' | cut -d' ' -f1")" = "$VERIFIER_CHECKSUM" ] \
  || die "the release verifier is not the one that was built"

# Both inputs are required and a rejection is fatal: a missing verifier or policy
# is exactly when the key needs proving, and warning past it is how a scraper
# built without the key silently ignores the signed policy.
$SSH "$HOST" "test -x '$VERIFIER'" || die "no verifier in $RELEASES/$REVISION"
$SSH "$HOST" "test -f '$HOST_ROOT/policy/wfm-policy.json'" || die "no policy at $HOST_ROOT/policy/wfm-policy.json"
if $SSH "$HOST" "'$VERIFIER' '$HOST_ROOT/policy/wfm-policy.json'"; then
  say "policy: revision-bound verifier accepts the live policy"
else
  # After a key rotation the live policy is still signed with the retired key,
  # and the box cannot pull its successor until this deploy installs the new
  # verifier. Accept that only when the published policy verifies with this
  # build's key as a newer revision of the live one - the check the puller
  # makes next. A build with the wrong key still fails here.
  $SSH "$HOST" "curl --fail --silent --show-error --location --max-time 15 --max-filesize 65536 -o '$STAGING/published-policy.json' '$PUBLISHED_POLICY_URL' && '$VERIFIER' '$STAGING/published-policy.json' '$HOST_ROOT/policy/wfm-policy.json'" \
    || die "the revision's verifier rejects the live policy - key mismatch"
  say "policy: the live policy predates a key rotation; its published successor verifies with this key"
fi

# ---- 7. activate the proven release ---------------------------------------
$SSH "$HOST" bash -s <<REMOTE_ACTIVATE
set -euo pipefail
# The running paths stay where the units expect them; the release directory is
# the rollback unit, not a new layout.
install -m 0755 "$RELEASES/$REVISION/tennoworth-pipeline" "/srv/wfm/bin/tennoworth-pipeline"
# The policy puller verifies each download with this binary, so it has to carry
# the same public key as the scraper it serves. It was once installed by hand and
# never updated; after a key rotation that copy would refuse every new policy.
install -m 0755 "$RELEASES/$REVISION/wfm-policy" "/srv/wfm/bin/wfm-policy"
install -m 0755 "$RELEASES/$REVISION/run-scrape.sh" "/srv/wfm/run-scrape.sh"
install -m 0644 "$RELEASES/$REVISION/tennoworth-scrape.service" /etc/systemd/system/tennoworth-scrape.service
install -m 0644 "$RELEASES/$REVISION/tennoworth-scrape.timer" /etc/systemd/system/tennoworth-scrape.timer
install -m 0755 "$RELEASES/$REVISION/host-check.sh" "/srv/wfm/host-check.sh"
install -m 0644 "$RELEASES/$REVISION/tennoworth-host-check.service" /etc/systemd/system/tennoworth-host-check.service
install -m 0644 "$RELEASES/$REVISION/tennoworth-host-check.timer" /etc/systemd/system/tennoworth-host-check.timer
# ProtectSystem=strict in the check's unit grants write access to exactly this
# path, and systemd refuses to start a unit whose ReadWritePaths does not exist.
# The private dashboard's publisher reads the report through its group; a box
# without the dashboard keeps it root's. Escaped so the lookup runs on the box.
install -d -m 0750 -o root -g "\$(getent group tennoworth-publisher >/dev/null && echo tennoworth-publisher || echo root)" "$HOST_ROOT/data/host-check"
systemctl daemon-reload
# A fresh box otherwise has the units and nothing scheduled: setup-container no
# longer enables the scrape timer, because this script owns the pipeline now.
# Arming the timer here with enable --now would reopen the window the hold above
# exists to close; the EXIT trap starts it once every check has passed, and the
# monitoring timers are deliberately armed after that.
systemctl enable tennoworth-scrape.timer
REMOTE_ACTIVATE

[ "$($SSH "$HOST" "sha256sum /srv/wfm/bin/tennoworth-pipeline | cut -d' ' -f1")" = "$CHECKSUM" ] \
  || die "the live binary is not the one that was built"

# The on-box archive receipt path is retired. A box deployed before that still
# has its pull units and the drop-in that was meant to detach the check from
# them - which systemd ignored, because an empty Wants= in a drop-in does not
# reset a dependency the base unit declares - so the pull kept running, failed
# every hour and alerted each time. Removing the units is what ends that.
$SSH "$HOST" "systemctl disable --now wfm-archive-receipt-pull.timer >/dev/null 2>&1 || true"
$SSH "$HOST" "rm -f /etc/systemd/system/wfm-archive-receipt-pull.service /etc/systemd/system/wfm-archive-receipt-pull.timer /srv/wfm/pull-archive-receipt.sh"
# The sweep's units, binary and job path units were named wfm-scrape and
# tennoworth-job-<id>.path before the units took the project's name. The hold
# stopped the old timer; left enabled it would schedule a second, binary-less
# sweep beside this one after the next reboot.
$SSH "$HOST" "systemctl disable wfm-scrape.timer tennoworth-job-scrape.path tennoworth-job-host-check.path >/dev/null 2>&1 || true"
$SSH "$HOST" "rm -f /etc/systemd/system/wfm-scrape.service /etc/systemd/system/wfm-scrape.timer /srv/wfm/bin/wfm-scrape /etc/systemd/system/tennoworth-job-scrape.path /etc/systemd/system/tennoworth-job-host-check.path"
# The observation corpus and its readiness check are retired. The corpus
# directory is left in place: removing it waits for a final backup.
$SSH "$HOST" "systemctl disable --now wfm-observations-check.timer tennoworth-job-readiness.path >/dev/null 2>&1 || true"
$SSH "$HOST" "rm -rf /etc/systemd/system/wfm-observations-check.service /etc/systemd/system/wfm-observations-check.timer /etc/systemd/system/wfm-observations-check.service.d /etc/systemd/system/tennoworth-job-readiness.path /etc/wfm-observations-check.env /srv/wfm/observations-check.sh /srv/wfm/jobs/readiness"
$SSH "$HOST" "systemctl daemon-reload"
$SSH "$HOST" "systemctl reset-failed wfm-archive-receipt-pull.service wfm-observations-check.service >/dev/null 2>&1 || true"

# ---- 8. record it ---------------------------------------------------------
$SSH "$HOST" "cat > '$HOST_ROOT/deployed.json'" <<RECORD
{
  "revision": "$REVISION",
  "built_at": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "sha256": "$CHECKSUM",
  "glibc_needed": "$WANTED_GLIBC",
  "glibc_on_box": "$BOX_GLIBC"
}
RECORD

# ---- 9. restore the schedule, then arm the host check ----------------------
# The monitor reads the deployment record and the sweep schedule, so it is armed
# only after those are settled: a persistent timer enabled earlier can fire into
# the intermediate state and report on a box that is still deploying.
# restore_timer is the same call the failure path makes, made deliberately here
# so the schedule is running before anything watches it - and disarms the trap
# so success does not start it twice.
restore_timer

$SSH "$HOST" "systemctl enable --now tennoworth-host-check.timer"
# The dashboard's pipeline jobs. The job path template ships with the
# dashboard, so only a box that has it gets the instances.
$SSH "$HOST" "if [ -f /etc/systemd/system/tennoworth-job@.path ]; then systemctl enable --now tennoworth-job@scrape.path tennoworth-job@host-check.path; fi"

# ---- 10. run the check once ------------------------------------------------
# A check that leaves no report at all means the installed units are broken, and
# a failing check means the box is not healthy enough to be left unattended.
# The start's own status is ignored because a not-ready check exits non-zero, so
# the report is the only verdict - and one left by an earlier run would stand in
# for a check that never wrote one. Remove it first.
$SSH "$HOST" "rm -f '$HOST_ROOT/data/host-check/report.json'" \
  || die "could not clear the previous host-check report"
$SSH "$HOST" "systemctl start tennoworth-host-check.service" || true
$SSH "$HOST" "test -s '$HOST_ROOT/data/host-check/report.json'" \
  || die "the host check produced no report - the installed units are not working"
READY="$($SSH "$HOST" "grep -o '\"ready\":[a-z ]*' '$HOST_ROOT/data/host-check/report.json' 2>/dev/null | head -1")"
case "$READY" in
  *true*)
    say "host check: healthy ($HOST_ROOT/data/host-check/report.json on the box)";;
  *)
    # The release is installed and the schedule is running; what failed is the
    # gate this deployment is required to pass.
    say "host check: NOT HEALTHY ($READY)"
    die "the host check does not pass on the box - fix what it reports rather than disabling the check (report at $HOST_ROOT/data/host-check/report.json)";;
esac

say "deployed $(git rev-parse --short "$REVISION"); rollback: install a previous $RELEASES/<rev>/ by hand and daemon-reload"
