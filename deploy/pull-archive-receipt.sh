#!/usr/bin/env bash
# Fetch the archive host's receipt onto the box, read-only there.
#
# The readiness check runs with no network access and must stay that way: its
# verdict is about the corpus on this box, and it has to be the same verdict
# whether or not a remote host is reachable. Retrieval is therefore a separate,
# box-owned step with its own failure mode and its own alert, and it runs before
# the check so the receipt it stores is the one the check reads.
#
# Everything the archive host does is read-only: this pulls one file. The write
# lands through a temp file and a rename, so a check never reads half a receipt.
#
# Environment:
#   ARCHIVE_RECEIPT_SOURCE  scp source of the receipt, e.g.
#                           archive@host:/srv/archive/receipt.jsonl
#                           (no default; without it there is nothing to fetch)
#   SCP                     scp command (default "scp")
#   SCP_OPTS / COMMAND_TIMEOUT  connection and whole-command bounds; an
#                           unreachable or stalled peer must fail this run rather
#                           than hold it open until the next elapse
#   OUT                     local destination (default
#                           /srv/wfm/data/observations-check/archive-receipt.jsonl)
set -euo pipefail

SOURCE="${ARCHIVE_RECEIPT_SOURCE:-}"
SCP="${SCP:-scp}"
SCP_OPTS="${SCP_OPTS:--o BatchMode=yes -o ConnectTimeout=15}"
COMMAND_TIMEOUT="${COMMAND_TIMEOUT:-120}"
OUT="${OUT:-/srv/wfm/data/observations-check/archive-receipt.jsonl}"

die() { printf 'ABORT: %s\n' "$*" >&2; exit 1; }

[ -n "$SOURCE" ] \
  || die "ARCHIVE_RECEIPT_SOURCE is not set - configure the archive host in /etc/wfm-archive-receipt.env"

mkdir -p "$(dirname "$OUT")"
tmp="$OUT.tmp"
rm -f "$tmp"
timeout "$COMMAND_TIMEOUT" $SCP $SCP_OPTS -q "$SOURCE" "$tmp" || die "could not fetch $SOURCE"

# A half-written or unrelated file must never become the evidence the check
# reads; the header is the cheapest thing that tells the two apart.
if ! head -1 "$tmp" 2>/dev/null | grep -q '"kind":"archive_receipt"'; then
  rm -f "$tmp"
  die "$SOURCE is not an archive receipt"
fi

mv "$tmp" "$OUT"
printf 'archive receipt: %s -> %s\n' "$SOURCE" "$OUT"
