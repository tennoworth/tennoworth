#!/usr/bin/env bash
# Deploy the wfm-scrape pipeline to the host directly, from a reviewed revision.
#
# The scrape is host-only infrastructure - the pipeline binary, the verifier
# built with it to prove its key, one driver script, the corpus readiness check
# and their units, and nothing a user installs contains any of them - so it does
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
#   8. the sweep schedule is restored, the declared preservation mode is written
#      where the check reads it, and only then is the monitor armed. The mode is
#      required: the corpus is protected by a host-level backup this box cannot
#      see, and the check reports it as declared rather than verified.
#
# Environment:
#   HOST          ssh target                (default wfm)
#   REVISION      commit to deploy          (default HEAD)
#   HOST_ROOT     deployment root on the box (default /srv/wfm)
#   SSH / SCP     ssh and scp commands  (default "ssh" and "scp")
#   DRY_RUN       1 builds and checks, then prints what it would install
#   PRESERVATION_MODE  required: external-backup. There is no default - a
#                 deploy that never says how the corpus is preserved must not
#                 come out ready claiming that it is.
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
REMOTE="${REMOTE:-github}"
# Where the corpus is preserved, declared explicitly by the operator. There is no
# default: the check turns this declaration into a ready report, so a deploy that
# stayed silent about preservation would produce a green box on a claim nobody
# made - exactly the evidence-free success the readiness check exists to refuse.
# external-backup is a daily host-level Proxmox backup of the whole container,
# verified by an operator-run job on another machine.
PRESERVATION_MODE="${PRESERVATION_MODE:-}"

: "${TENNOWORTH_WFM_POLICY_PUBLIC_KEY:?set TENNOWORTH_WFM_POLICY_PUBLIC_KEY - without it the deployed scraper silently ignores the signed policy}"

say() { printf '%s\n' "$*"; }
die() { printf 'ABORT: %s\n' "$*" >&2; exit 1; }

# An omitted and an empty value are the same refusal: neither declares anything.
[ -n "$PRESERVATION_MODE" ] \
  || die "PRESERVATION_MODE is not declared - set it explicitly to external-backup"
case "$PRESERVATION_MODE" in
  external-backup) ;;
  *) die "PRESERVATION_MODE must be external-backup (got '$PRESERVATION_MODE')";;
esac

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
ARTIFACT="$TARGET_DIR/release/wfm-scrape"
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
#   systemctl list-timers wfm-scrape.timer
#   systemctl start wfm-scrape.timer
# A SIGKILL or a host power loss runs none of this; the unit stays enabled, so a
# reboot re-arms it through timers.target even then.
TIMER_HELD=1
restore_timer() {
  [ "${TIMER_HELD:-0}" = 1 ] || return 0
  TIMER_HELD=0
  # Return non-zero when the restore does not take effect. In the EXIT trap the
  # status is discarded, but the success path calls this explicitly and a deploy
  # that leaves the box with no schedule must not report success.
  $SSH "$HOST" "systemctl start wfm-scrape.timer" && return 0
  say "WARNING: restore the timer by hand and confirm it: systemctl list-timers wfm-scrape.timer; systemctl start wfm-scrape.timer"
  return 1
}
trap restore_timer EXIT

$SSH "$HOST" "systemctl stop wfm-scrape.timer >/dev/null 2>&1 || true; ! systemctl is-active --quiet wfm-scrape.timer" \
  || die "could not stop wfm-scrape.timer; refusing to install under a live schedule"

# Stopping the timer does not stop a sweep a previous elapse already started.
# `systemctl is-active` exits non-zero for inactive and failed, so its exit
# status cannot separate "the box says terminal" from "the query never ran".
# Read the state string and accept only what systemd can report for a oneshot: an
# empty or unrecognised result is a dead connection or a missing systemctl, and
# treating that as idle is how this check fails open.
sweep_state() {
  local state
  if ! state="$($SSH "$HOST" "systemctl is-active wfm-scrape.service" 2>/dev/null)"; then
    : # inactive and failed exit non-zero; the state string below decides
  fi
  case "$state" in
    active|activating|deactivating|inactive|failed) printf '%s\n' "$state" ;;
    *) die "could not read wfm-scrape.service state (got '${state:-nothing}'); refusing to install" ;;
  esac
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
$SCP -q "$ARTIFACT" "$HOST:$STAGING/wfm-scrape"
$SCP -q "$VERIFIER_ARTIFACT" "$HOST:$STAGING/wfm-policy"
$SCP -q deploy/run-scrape.sh deploy/wfm-scrape.service deploy/wfm-scrape.timer "$HOST:$STAGING/"
$SCP -q deploy/observations-check.sh deploy/wfm-observations-check.service deploy/wfm-observations-check.timer "$HOST:$STAGING/"

# ---- 6. install and prove the release before the live paths move -----------
# The live paths keep running the previous release until this one is proven on
# the box. A rejected policy has to leave what is deployed exactly as it was,
# not a half-swapped tree that the restored timer then runs against.
$SSH "$HOST" bash -s <<REMOTE_RELEASE
set -euo pipefail
install -d -m 0755 -o root -g root "$RELEASES/$REVISION"
install -m 0755 "$STAGING/wfm-scrape" "$RELEASES/$REVISION/wfm-scrape"
install -m 0755 "$STAGING/wfm-policy" "$RELEASES/$REVISION/wfm-policy"
install -m 0755 "$STAGING/run-scrape.sh" "$RELEASES/$REVISION/run-scrape.sh"
install -m 0644 "$STAGING/wfm-scrape.service" "$RELEASES/$REVISION/wfm-scrape.service"
install -m 0644 "$STAGING/wfm-scrape.timer" "$RELEASES/$REVISION/wfm-scrape.timer"
install -m 0755 "$STAGING/observations-check.sh" "$RELEASES/$REVISION/observations-check.sh"
install -m 0644 "$STAGING/wfm-observations-check.service" "$RELEASES/$REVISION/wfm-observations-check.service"
install -m 0644 "$STAGING/wfm-observations-check.timer" "$RELEASES/$REVISION/wfm-observations-check.timer"
REMOTE_RELEASE

SCRAPER="$RELEASES/$REVISION/wfm-scrape"
VERIFIER="$RELEASES/$REVISION/wfm-policy"

$SSH "$HOST" "'$SCRAPER' 2>&1 | grep -q 'usage: wfm-scrape'" \
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
$SSH "$HOST" "'$VERIFIER' '$HOST_ROOT/policy/wfm-policy.json'" || die "the revision's verifier rejects the live policy - key mismatch"
say "policy: revision-bound verifier accepts the live policy"

# ---- 7. activate the proven release ---------------------------------------
$SSH "$HOST" bash -s <<REMOTE_ACTIVATE
set -euo pipefail
# The running paths stay where the units expect them; the release directory is
# the rollback unit, not a new layout.
install -m 0755 "$RELEASES/$REVISION/wfm-scrape" "/srv/wfm/bin/wfm-scrape"
install -m 0755 "$RELEASES/$REVISION/run-scrape.sh" "/srv/wfm/run-scrape.sh"
install -m 0644 "$RELEASES/$REVISION/wfm-scrape.service" /etc/systemd/system/wfm-scrape.service
install -m 0644 "$RELEASES/$REVISION/wfm-scrape.timer" /etc/systemd/system/wfm-scrape.timer
install -m 0755 "$RELEASES/$REVISION/observations-check.sh" "/srv/wfm/observations-check.sh"
install -m 0644 "$RELEASES/$REVISION/wfm-observations-check.service" /etc/systemd/system/wfm-observations-check.service
install -m 0644 "$RELEASES/$REVISION/wfm-observations-check.timer" /etc/systemd/system/wfm-observations-check.timer
# ProtectSystem=strict in both new units grants write access to exactly this
# path, and systemd refuses to start a unit whose ReadWritePaths does not exist.
install -d -m 0750 -o root -g root "$HOST_ROOT/data/observations-check"
systemctl daemon-reload
# A fresh box otherwise has the units and nothing scheduled: setup-container no
# longer enables the scrape timer, because this script owns the pipeline now.
# Arming the timer here with enable --now would reopen the window the hold above
# exists to close; the EXIT trap starts it once every check has passed, and the
# monitoring timers are deliberately armed after that.
systemctl enable wfm-scrape.timer
REMOTE_ACTIVATE

[ "$($SSH "$HOST" "sha256sum /srv/wfm/bin/wfm-scrape | cut -d' ' -f1")" = "$CHECKSUM" ] \
  || die "the live binary is not the one that was built"

# The on-box archive receipt path is retired. A box deployed before that still
# has its pull units and the drop-in that was meant to detach the check from
# them - which systemd ignored, because an empty Wants= in a drop-in does not
# reset a dependency the base unit declares - so the pull kept running, failed
# every hour and alerted each time. Removing the units is what ends that.
$SSH "$HOST" "systemctl disable --now wfm-archive-receipt-pull.timer >/dev/null 2>&1 || true"
$SSH "$HOST" "rm -f /etc/systemd/system/wfm-archive-receipt-pull.service /etc/systemd/system/wfm-archive-receipt-pull.timer /etc/systemd/system/wfm-observations-check.service.d/preservation.conf /srv/wfm/pull-archive-receipt.sh"
$SSH "$HOST" "rmdir /etc/systemd/system/wfm-observations-check.service.d 2>/dev/null || true"
$SSH "$HOST" "systemctl daemon-reload"
$SSH "$HOST" "systemctl reset-failed wfm-archive-receipt-pull.service >/dev/null 2>&1 || true"

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

# ---- 9. restore the schedule, declare the mode, then arm the monitor --------
# The monitor reads the deployment record and the sweep schedule, so it is armed
# only after those are settled: a persistent timer enabled earlier can fire into
# the intermediate state and report on a box that is still deploying.
# restore_timer is the same call the failure path makes, made deliberately here
# so the schedule is running before anything watches it - and disarms the trap
# so success does not start it twice.
restore_timer

# The mode is a deployment fact the check cannot infer, so it is written where
# the unit reads it. An undeclared mode is a not-ready verdict by design.
$SSH "$HOST" "cat > /etc/wfm-observations-check.env" <<ENV
# Set by scripts/deploy-scrape-host.sh. external-backup declares that the corpus
# is protected by host-level Proxmox backups of this container, verified by an
# operator-run job elsewhere.
OBSERVATIONS_PRESERVATION=$PRESERVATION_MODE
ENV
$SSH "$HOST" "systemctl enable --now wfm-observations-check.timer"

# ---- 10. run the check once ------------------------------------------------
# A check that leaves no report at all means the installed units are broken, and
# a check that is not ready means the box is not ready to be left unattended.
$SSH "$HOST" "systemctl start wfm-observations-check.service" || true
$SSH "$HOST" "test -s '$HOST_ROOT/data/observations-check/report.json'" \
  || die "the readiness check produced no report - the installed units are not working"
READY="$($SSH "$HOST" "grep -o '\"ready\":[a-z ]*' '$HOST_ROOT/data/observations-check/report.json' 2>/dev/null | head -1")"
case "$READY" in
  *true*)
    say "readiness: ready ($HOST_ROOT/data/observations-check/report.json on the box, preservation=$PRESERVATION_MODE)";;
  *)
    # The release is installed and the schedule is running; what failed is the
    # gate this deployment is required to pass.
    say "readiness: NOT READY ($READY)"
    die "the readiness check does not pass on the box (preservation=$PRESERVATION_MODE) - fix what it reports rather than disabling the check (report at $HOST_ROOT/data/observations-check/report.json)";;
esac

say "deployed $(git rev-parse --short "$REVISION"); rollback: install a previous $RELEASES/<rev>/ by hand and daemon-reload"
