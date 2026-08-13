#!/usr/bin/env bash
# Local Linux UI smoke gate: run the REAL desktop app under a virtual display
# with TENNOWORTH_PROBE=1 and gate the probe report (console/CSP violations,
# SPA mount, Tauri IPC mode, scan CTA). The remote ui-smoke.yml does the same
# on Windows; this is the version to run on a Linux box BEFORE pushing - it
# catches the "builds fine, feature silently no-ops" class that static gates
# cannot see, without paying for a Windows build.
#
# Needs: xvfb-run (Arch: sudo pacman -S xorg-server-xvfb; Debian/Ubuntu:
# sudo apt install xvfb), the repo's normal build deps, and (only if you want
# the scan leg to attempt real memory reads) a one-time
#   sudo setcap cap_sys_ptrace=eip companion/target/debug/tennoworth-desktop
# Without a running game the scan fails gracefully either way - the gate does
# NOT require a scan hit.
#
# The frontend and binary are rebuilt every run (cargo is incremental): a
# gate that tests stale artifacts is worse than no gate.
#
# Usage: bash scripts/probe-smoke-linux.sh
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

command -v xvfb-run >/dev/null || {
  echo "probe-smoke: xvfb-run not found - install a virtual X server:" >&2
  echo "  sudo pacman -S xorg-server-xvfb   # Arch" >&2
  echo "  sudo apt install xvfb             # Debian/Ubuntu" >&2
  exit 1
}

echo "probe-smoke: building dist-desktop (embedded at compile time)..."
(cd prototype && bun run build:desktop)
echo "probe-smoke: building the desktop binary..."
(cd companion && cargo build -p tennoworth-desktop)

BIN=companion/target/debug/tennoworth-desktop
REPORT="$(mktemp -t probe-smoke-XXXXXX.json)"
trap 'rm -f "$REPORT"' EXIT

# The probe auto-exits after the FINAL report; timeout is a backstop so a
# hung webview fails the gate instead of hanging the pre-push flow.
set +e
TENNOWORTH_PROBE=1 TENNOWORTH_PROBE_OUT="$REPORT" timeout 180 xvfb-run -a "$BIN" \
  >/tmp/probe-smoke-stdout.log 2>&1
RC=$?
set -e

case "$RC" in
  0) ;;
  124)
    echo "probe-smoke: the app did not exit within 180s under xvfb - hung webview or probe failure." >&2
    tail -20 /tmp/probe-smoke-stdout.log >&2
    exit 1
    ;;
  *)
    echo "probe-smoke: the app exited $RC under xvfb." >&2
    tail -20 /tmp/probe-smoke-stdout.log >&2
    exit 1
    ;;
esac

node scripts/check-probe-report.mjs "$REPORT"
echo "probe-smoke: OK - the real app ran the full UI probe under xvfb and passed."
