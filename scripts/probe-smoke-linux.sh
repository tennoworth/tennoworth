#!/usr/bin/env bash
# Build and run the real desktop probe in an isolated display and D-Bus session.
# A separate bus prevents an already-running app from intercepting this launch.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

ARTIFACT=""
if [[ "${1:-}" == "--artifact" ]]; then
  ARTIFACT="${2:-}"
  if [[ -z "$ARTIFACT" || $# -ne 2 ]]; then
    echo "usage: $0 [--artifact <AppImage>]" >&2
    exit 2
  fi
elif [[ $# -ne 0 ]]; then
  echo "usage: $0 [--artifact <AppImage>]" >&2
  exit 2
fi

for tool in xvfb-run dbus-run-session; do
  command -v "$tool" >/dev/null || {
    echo "probe-smoke: $tool is required (install Xvfb and D-Bus)." >&2
    exit 1
  }
done

if [[ -n "$ARTIFACT" ]]; then
  [[ "$ARTIFACT" = /* ]] || ARTIFACT="$ROOT/$ARTIFACT"
  if [[ ! -s "$ARTIFACT" ]]; then
    echo "probe-smoke: artifact is absent or empty: $ARTIFACT" >&2
    exit 2
  fi
  BIN="$(realpath "$ARTIFACT")"
  chmod +x "$BIN"
  # CI runners do not provide FUSE. The AppImage runtime extracts and launches
  # the final package contents through AppRun when this is set.
  export APPIMAGE_EXTRACT_AND_RUN=1
else
  # The embedded SPA must be rebuilt before Cargo, even when a binary exists.
  (cd frontend && bun run build:desktop)
  (cd rust && cargo build -p tennoworth-desktop)
  TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/rust/target}"
  if [[ "$TARGET_DIR" != /* ]]; then TARGET_DIR="$ROOT/rust/$TARGET_DIR"; fi
  BIN="$TARGET_DIR/debug/tennoworth-desktop"
fi
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
  TENNOWORTH_JWT_PATH="$SCRATCH/wfm-jwt.enc" TENNOWORTH_PENDING_PATH="$SCRATCH/pending-plan.json" \
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
