#!/usr/bin/env bash
# Install the archive pieces on the archive host, and prove they are installed.
#
# Run this on the archive host, not on the box: the box only pulls a receipt.
# This host runs the archive job, and `OnFailure=wfm-alert@%n.service` in its
# units does nothing at all unless the alert template and handler are installed
# here too - a reference to a unit is not a provisioned unit, and an alert that
# cannot run is the failure mode this monitoring exists to remove. So the script
# installs every piece and then verifies the result instead of assuming it.
#
# Run as root, from the checkout:
#   scripts/install-archive-host.sh
#
# Environment (all overridable; the defaults are the host layout):
#   PREFIX          where the scripts land        (default /usr/local/bin)
#   UNIT_DIR        systemd unit directory        (default /etc/systemd/system)
#   ENV_FILE        archive run configuration     (default /etc/tennoworth-archive.env)
#   ALERT_ENV_FILE  alert handler configuration   (default /etc/wfm-alert.env)
#   ALERT_LOG       where this host records alerts (default
#                   /var/log/tennoworth-archive-alerts.log)
#   REPO_ROOT       checkout to install from      (default this script's parent)
#   SYSTEMCTL / SYSTEMD_ANALYZE   commands        (default systemctl / systemd-analyze)
set -euo pipefail

REPO_ROOT="${REPO_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"
PREFIX="${PREFIX:-/usr/local/bin}"
UNIT_DIR="${UNIT_DIR:-/etc/systemd/system}"
ENV_FILE="${ENV_FILE:-/etc/tennoworth-archive.env}"
ALERT_ENV_FILE="${ALERT_ENV_FILE:-/etc/wfm-alert.env}"
ALERT_LOG="${ALERT_LOG:-/var/log/tennoworth-archive-alerts.log}"
SYSTEMCTL="${SYSTEMCTL:-systemctl}"
SYSTEMD_ANALYZE="${SYSTEMD_ANALYZE:-systemd-analyze}"

say() { printf '%s\n' "$*"; }
die() { printf 'ABORT: %s\n' "$*" >&2; exit 1; }

for file in scripts/archive-observations.sh deploy/alert.sh deploy/wfm-alert@.service \
  deploy/archive-observations.service deploy/archive-observations.timer; do
  [ -f "$REPO_ROOT/$file" ] || die "missing $REPO_ROOT/$file - run this from the checkout"
done
[ "$(id -u)" = 0 ] || die "run as root: this installs into $PREFIX and $UNIT_DIR"

install -d -m 0755 "$PREFIX" "$UNIT_DIR"
install -m 0755 "$REPO_ROOT/scripts/archive-observations.sh" "$PREFIX/archive-observations.sh"
install -m 0755 "$REPO_ROOT/deploy/alert.sh" "$PREFIX/alert.sh"
install -m 0644 "$REPO_ROOT/deploy/archive-observations.service" "$UNIT_DIR/archive-observations.service"
install -m 0644 "$REPO_ROOT/deploy/archive-observations.timer" "$UNIT_DIR/archive-observations.timer"
# The template is installed with its handler path rewritten: the box keeps its
# handler under /srv/wfm, and this host has no /srv/wfm. Without the rewrite the
# template verifies as broken and OnFailure would have nothing to execute.
sed "s#/srv/wfm/alert.sh#$PREFIX/alert.sh#" "$REPO_ROOT/deploy/wfm-alert@.service" \
  > "$UNIT_DIR/wfm-alert@.service"

# The archive run's HOST/SSH/DEST live here. Never overwritten: the destination
# is state, and re-running this must not silently repoint it.
if [ ! -f "$ENV_FILE" ]; then
  cat > "$ENV_FILE" <<'ENV'
# Configuration for archive-observations.service.
# HOST=wfm
# SSH='ssh -F /path/to/config'
# DEST=$HOME/.local/share/tennoworth/observations-archive
ENV
  chmod 0644 "$ENV_FILE"
  say "wrote $ENV_FILE (set HOST and DEST for this host)"
else
  say "keeping existing $ENV_FILE"
fi

# The alert handler has no logging destination of its own on this host: the box's
# default path does not exist here, so it is configured rather than assumed.
if [ ! -f "$ALERT_ENV_FILE" ]; then
  cat > "$ALERT_ENV_FILE" <<ENV
# Configuration for the alert handler (deploy/alert.sh).
WFM_ALERT_LOG=$ALERT_LOG
# ALERT_WEBHOOK_URL=https://...
ENV
  chmod 0644 "$ALERT_ENV_FILE"
  say "wrote $ALERT_ENV_FILE"
elif ! grep -q '^WFM_ALERT_LOG=' "$ALERT_ENV_FILE"; then
  say "WARNING: $ALERT_ENV_FILE sets no WFM_ALERT_LOG, so alerts from this host are journal-only"
fi

"$SYSTEMCTL" daemon-reload

# ---- verify, do not assume --------------------------------------------------
grep -q "$PREFIX/alert.sh" "$UNIT_DIR/wfm-alert@.service" \
  || die "the installed alert template does not point at $PREFIX/alert.sh"
"$SYSTEMD_ANALYZE" verify "$UNIT_DIR/wfm-alert@.service" "$UNIT_DIR/archive-observations.service" "$UNIT_DIR/archive-observations.timer" \
  || die "the installed units do not verify"

# Run the handler exactly as systemd would, and require it to have recorded
# something: the point is a working alert path, not a unit file that looks right.
WFM_ALERT_ENV="$ALERT_ENV_FILE" "$PREFIX/alert.sh" archive-observations.service >/dev/null 2>&1 \
  || die "the alert handler failed the unit name the archive units pass it"
recorded_log="$ALERT_LOG"
if [ -r "$ALERT_ENV_FILE" ]; then
  configured="$(grep -h '^WFM_ALERT_LOG=' "$ALERT_ENV_FILE" 2>/dev/null | tail -1 | cut -d= -f2- || true)"
  [ -n "$configured" ] && recorded_log="$configured"
fi
[ -s "$recorded_log" ] || die "the alert handler recorded nothing in $recorded_log"

"$SYSTEMCTL" enable --now archive-observations.timer
[ "$("$SYSTEMCTL" is-enabled archive-observations.timer 2>/dev/null || true)" = enabled ] \
  || die "archive-observations.timer is not enabled"

say "archive host ready: $PREFIX/archive-observations.sh, units in $UNIT_DIR, alerts recorded in $recorded_log"
