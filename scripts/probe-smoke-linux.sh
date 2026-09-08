#!/usr/bin/env bash
# Build and run the real desktop probe in an isolated display and D-Bus session.
# A separate bus prevents an already-running app from intercepting this launch.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

for tool in xvfb-run dbus-run-session; do
  command -v "$tool" >/dev/null || {
    echo "probe-smoke: $tool is required (install Xvfb and D-Bus)." >&2
    exit 1
  }
done

# The embedded SPA must be rebuilt before Cargo, even when a binary exists.
(cd prototype && bun run build:desktop)
(cd companion && cargo build -p tennoworth-desktop)
TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/companion/target}"
if [[ "$TARGET_DIR" != /* ]]; then TARGET_DIR="$ROOT/companion/$TARGET_DIR"; fi
BIN="$TARGET_DIR/debug/tennoworth-desktop"
EVIDENCE="${TENNOWORTH_PROBE_EVIDENCE_DIR:-$(mktemp -d -t probe-evidence-XXXXXX)}"
mkdir -p "$EVIDENCE"
EVIDENCE="$(cd "$EVIDENCE" && pwd)"
REPORT="$EVIDENCE/probe-report.json"
LOG="$EVIDENCE/probe-stdout.log"
# A restored real inventory would bypass onboarding and invalidate the probe.
SCRATCH="$(mktemp -d -t probe-xdg-XXXXXX)"
trap 'rm -rf "$SCRATCH"' EXIT
rm -f "$REPORT"

set +e
XDG_DATA_HOME="$SCRATCH" TENNOWORTH_PROBE=1 TENNOWORTH_PROBE_OUT="$REPORT" \
  timeout 180 dbus-run-session -- xvfb-run -a "$BIN" >"$LOG" 2>&1
RC=$?
set -e
if [[ "$RC" -ne 0 ]]; then
  echo "probe-smoke: app exited $RC; evidence: $EVIDENCE" >&2
  tail -20 "$LOG" >&2
  exit "$RC"
fi
bun scripts/check-probe-report.ts "$REPORT"
echo "probe-smoke: OK; evidence: $EVIDENCE"
