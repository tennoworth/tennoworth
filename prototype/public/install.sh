#!/usr/bin/env sh
# wfm-fetch-inventory — Linux installer.
#
# Pipe-curl idiom: `curl -fsSL <url> | sh`. The script downloads the
# latest released binary from GitHub, places it on PATH, and verifies
# the checksum from the matching SHA256SUMS file in the release.

set -eu

# Configure these once before publishing the first release.
REPO="${WFMINV_REPO:-OWNER/REPO}"
BIN_NAME="wfm-fetch-inventory"
ASSET="${BIN_NAME}-linux-x86_64"
DEST="${WFMINV_DEST:-$HOME/.local/bin}"

case "$(uname -s)" in
  Linux) ;;
  *) echo "This installer is for Linux. For Windows, see the website."; exit 1 ;;
esac
case "$(uname -m)" in
  x86_64|amd64) ;;
  *) echo "Unsupported architecture: $(uname -m). Only x86_64 binaries are published."; exit 1 ;;
esac

mkdir -p "$DEST"

# WFMINV_BASE_URL lets us point at a local server during development. In
# production it stays unset and we fall through to GitHub releases. We
# require https on this var because a plaintext base URL would let a
# MITM swap both the binary AND the matching SHA256SUMS, defeating the
# verification below. Dev usage can override REPO instead.
if [ -n "${WFMINV_BASE_URL:-}" ]; then
  case "$WFMINV_BASE_URL" in
    https://*) ;;
    *) echo "WFMINV_BASE_URL must start with https:// (got: $WFMINV_BASE_URL)"; exit 1 ;;
  esac
  URL="$WFMINV_BASE_URL/$ASSET"
  SUMS_URL="$WFMINV_BASE_URL/SHA256SUMS"
else
  URL="https://github.com/$REPO/releases/latest/download/$ASSET"
  SUMS_URL="https://github.com/$REPO/releases/latest/download/SHA256SUMS"
fi
TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT

echo "→ Downloading $ASSET"
if ! curl -fL --progress-bar "$URL" -o "$TMPDIR/$ASSET"; then
  echo "Download failed. Check the release exists at $URL"; exit 1
fi

# Verify checksum — hard requirement. A missing SHA256SUMS means we
# cannot prove the binary is the one we built, so we refuse to install
# it. SECURITY.md commits to automatic verification; this is what
# implements that commitment.
if ! curl -fsSL "$SUMS_URL" -o "$TMPDIR/SHA256SUMS"; then
  echo "Could not fetch SHA256SUMS from $SUMS_URL — refusing to install" >&2
  echo "an unverified binary. Aborting." >&2
  exit 1
fi
expected=$(grep " $ASSET\$" "$TMPDIR/SHA256SUMS" | awk '{print $1}')
actual=$(sha256sum "$TMPDIR/$ASSET" | awk '{print $1}')
if [ -z "$expected" ]; then
  echo "SHA256SUMS exists but has no entry for $ASSET. Aborting." >&2
  exit 1
fi
if [ "$expected" != "$actual" ]; then
  echo "Checksum mismatch:" >&2
  echo "  expected $expected" >&2
  echo "  actual   $actual" >&2
  echo "Aborting install." >&2
  exit 1
fi
echo "→ Checksum OK"

chmod +x "$TMPDIR/$ASSET"
mv "$TMPDIR/$ASSET" "$DEST/$BIN_NAME"

echo "→ Installed: $DEST/$BIN_NAME"

case ":$PATH:" in
  *":$DEST:"*) ;;
  *)
    echo
    echo "⚠  $DEST is not on your PATH. Add this to your shell rc:"
    echo "    export PATH=\"$DEST:\$PATH\""
    ;;
esac

cat <<EOF

Next steps
  1. Start Warframe and log past the title screen.
  2. Open the trade or profile screen once (forces an auth call).
  3. Grant ptrace permission once (recommended — then no sudo, ever):
       sudo setcap cap_sys_ptrace=eip "$DEST/$BIN_NAME"
       $BIN_NAME
     Rather not? Run a single fetch with sudo instead:
       sudo $BIN_NAME
  4. inventory.json lands in ~/Downloads — drop it into the web UI.

Re-running this installer (an upgrade) replaces the binary and clears
the capability — re-run the setcap line above afterwards.
EOF
