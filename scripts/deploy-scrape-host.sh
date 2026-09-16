#!/usr/bin/env bash
# Deploy the wfm-scrape pipeline to the host directly, from a reviewed revision.
#
# The scrape is host-only infrastructure - one binary, one driver script, two
# units, and nothing a user installs contains any of them - so it does not need
# the GitHub release relay the web bundle and desktop app need. Commits still
# live in the monorepo and are still reviewed; this script installs an immutable
# revision of them, and nothing else.
#
# Order of operations, all of which are load-bearing:
#   1. the tree is clean and the revision is already on the remote,
#   2. the crate's own gates pass (they are the only thing that catches field
#      drift and crash-family regressions in this pipeline),
#   3. the artifact links no glibc newer than the box's,
#   4. the box is idle, so one sweep cannot span two releases,
#   5. the whole release is installed in one window, with the previous one kept,
#   6. the deployed verifier accepts the deployed policy - a binary built
#      without the public key silently ignores signed policy, which is exactly
#      the failure this check exists for,
#   7. /srv/wfm/deployed.json records what ran and what it was checked against.
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
say "deploying $(git rev-parse --short "$REVISION") from ${BASH_REMATCH[0]:-$(git branch --show-current 2>/dev/null || echo 'a detached HEAD')}"

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
# Honor CARGO_TARGET_DIR: CI and sandboxes set it, and the artifact is not under
# rust/target when they do.
TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/rust/target}"
ARTIFACT="$TARGET_DIR/release/wfm-scrape"
[ -x "$ARTIFACT" ] || die "no artifact at $ARTIFACT"

# The box is newer than the build host in practice (Debian 13 today), but glibc
# has no forward compatibility: a binary needing a symbol newer than the box's
# fails at start. Compare symbol versions rather than trusting the build host.
BOX_GLIBC="$($SSH "$HOST" "ldd --version | head -1 | grep -oE '[0-9]+\.[0-9]+$'")"
[ -n "$BOX_GLIBC" ] || die "could not read the box's glibc version"
WANTED_GLIBC="$(objdump -T "$ARTIFACT" | grep -oE 'GLIBC_[0-9]+\.[0-9]+' | sed 's/GLIBC_//' | sort -V | tail -1)"
[ -n "$WANTED_GLIBC" ] || die "could not read the artifact's required glibc symbols"
newest="$(printf '%s\n%s\n' "$BOX_GLIBC" "$WANTED_GLIBC" | sort -V | tail -1)"
[ "$newest" = "$BOX_GLIBC" ] \
  || die "artifact needs GLIBC_$WANTED_GLIBC but the box has $BOX_GLIBC"
say "glibc: needs <= $WANTED_GLIBC, box has $BOX_GLIBC"

CHECKSUM="$(sha256sum "$ARTIFACT" | cut -d' ' -f1)"
say "sha256 $CHECKSUM"

if [ "$DRY_RUN" = 1 ]; then
  say "DRY RUN - would install to $RELEASES/$REVISION and record $HOST_ROOT/deployed.json"
  exit 0
fi

# ---- 4. the box is idle ----------------------------------------------------
# Installing file by file is not atomic across the release, and a sweep runs for
# the better part of an hour every two. Rather than let one sweep span two
# binaries, refuse while anything is running.
state="$($SSH "$HOST" "systemctl is-active wfm-scrape.service" || true)"
[ "$state" != "activating" ] && [ "$state" != "active" ] \
  || die "a sweep is $state; deploy between sweeps (systemctl list-timers wfm-scrape.timer)"

# ---- 5. stage, then install the whole release in one window ---------------
say "staging to $STAGING"
$SSH "$HOST" "install -d -m 0755 -o root -g root '$STAGING'"
$SCP -q "$ARTIFACT" "$HOST:$STAGING/wfm-scrape"
$SCP -q deploy/run-scrape.sh deploy/wfm-scrape.service deploy/wfm-scrape.timer "$HOST:$STAGING/"

$SSH "$HOST" bash -s <<REMOTE_INSTALL
set -euo pipefail
install -d -m 0755 -o root -g root "$RELEASES/$REVISION"
install -m 0755 "$STAGING/wfm-scrape" "$RELEASES/$REVISION/wfm-scrape"
install -m 0755 "$STAGING/run-scrape.sh" "$RELEASES/$REVISION/run-scrape.sh"
install -m 0644 "$STAGING/wfm-scrape.service" "$RELEASES/$REVISION/wfm-scrape.service"
install -m 0644 "$STAGING/wfm-scrape.timer" "$RELEASES/$REVISION/wfm-scrape.timer"

# The running paths stay where the units expect them; the release directory is
# the rollback unit, not a new layout.
install -m 0755 "$RELEASES/$REVISION/wfm-scrape" "/srv/wfm/bin/wfm-scrape"
install -m 0755 "$RELEASES/$REVISION/run-scrape.sh" "/srv/wfm/run-scrape.sh"
install -m 0644 "$RELEASES/$REVISION/wfm-scrape.service" /etc/systemd/system/wfm-scrape.service
install -m 0644 "$RELEASES/$REVISION/wfm-scrape.timer" /etc/systemd/system/wfm-scrape.timer
systemctl daemon-reload
# A fresh box otherwise has the units and nothing scheduled: setup-container no
# longer enables the scrape timer, because this script owns the pipeline now.
systemctl enable --now wfm-scrape.timer
REMOTE_INSTALL

# ---- 6. the deployed pair actually works ----------------------------------
$SSH "$HOST" "'/srv/wfm/bin/wfm-scrape' 2>&1 | grep -q 'usage: wfm-scrape'" \
  || die "the installed binary does not run on the box"
[ "$($SSH "$HOST" "sha256sum /srv/wfm/bin/wfm-scrape | cut -d' ' -f1")" = "$CHECKSUM" ] \
  || die "the installed binary is not the one that was built"

# The verifier embeds the same public key the build used, so a successful run
# proves both that the key reached the binary and that the live policy verifies
# under it. A missing key would have fallen back to compiled defaults silently.
if $SSH "$HOST" "test -x /srv/wfm/bin/wfm-policy && test -f /srv/wfm/policy/wfm-policy.json"; then
  $SSH "$HOST" "/srv/wfm/bin/wfm-policy /srv/wfm/policy/wfm-policy.json" \
    || die "the deployed verifier rejects the deployed policy - key mismatch"
  say "policy: deployed verifier accepts the live policy"
else
  say "policy: no verifier or policy on the box yet; the built-in key is unproven"
fi

# ---- 7. record it ---------------------------------------------------------
$SSH "$HOST" "cat > '$HOST_ROOT/deployed.json'" <<RECORD
{
  "revision": "$REVISION",
  "built_at": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "sha256": "$CHECKSUM",
  "glibc_needed": "$WANTED_GLIBC",
  "glibc_on_box": "$BOX_GLIBC"
}
RECORD

say "deployed $(git rev-parse --short "$REVISION"); rollback: install a previous $RELEASES/<rev>/ by hand and daemon-reload"
