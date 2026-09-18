#!/usr/bin/env bash
# Pull the newest Proxmox LXC backup off the node and prove it is restorable.
#
# Preservation for the observation corpus is a host-level vzdump of the whole
# container: a daily, snapshot-mode, zstd backup that lands on the node and is
# pruned to the last two. A copy of that archive is evidence only while it can
# still be read - the transfer can truncate it, the compression stream can be
# damaged, the tar listing can be unusable, or the corpus log inside it can be
# something other than the run header a replay needs. A bad copy filed as good
# is worse than no copy, because the loss surfaces when the backup is needed.
# Every run therefore verifies what it received - the zstd stream, the tar
# listing, and the newest corpus log's own first record read back out of the
# archive - and only then records a receipt of what was proven and when.
#
# The fetch is a download, not a trust: the new copy lands as a hidden .part and
# is moved into place only after it verifies, so a bad nightly can never replace
# the last proven archive, and the manifest is rewritten only on success so a
# failed run leaves the previous receipt exactly as it was.
#
# Read-only on the node: this copies two files out of the dump directory and
# never touches the container, the backup job, or the hypervisor's own storage.
#
# Environment:
#   HOST          ssh alias of the Proxmox node           (required)
#   VMID          container id whose dumps to pull        (required)
#   DUMP_DIR      vzdump directory on the node            (default /var/lib/vz/dump)
#   DEST          local destination directory             (default $HOME/backups/tennoworth)
#   KEEP          local archives to retain, newest first  (default 30)
#   SSH / SCP     ssh and scp commands                    (default "ssh" and "scp")
#   SSH_OPTS / SCP_OPTS / COMMAND_TIMEOUT
#                 connection and whole-command bounds for unattended runs
#   CORPUS_MATCH  extended regex a tar member must match to be a corpus log
#                 (default 'observations/sweep-[^/]*\.jsonl$')
#
# Requires python3 to parse the corpus run header as JSON. A record is only the
# header a replay can use if it parses; a hand-rolled field match cannot tell a
# complete record from a truncated one.
#
# Usage: check-lxc-backup.sh [--dry-run]
#
# Exit status: 0 the newest archive is present and proven, 1 a refusal or a
# verification failure (the previous receipt and archives are untouched), 2 bad
# usage.
set -euo pipefail

usage() {
  cat <<'USAGE'
usage: check-lxc-backup.sh [--dry-run]

  --dry-run   report the archive that would be fetched and verified, then stop
              without downloading anything

Environment:
  HOST        ssh alias of the Proxmox node (required)
  VMID        container id whose dumps to pull (required)
  DUMP_DIR    vzdump directory on the node (default /var/lib/vz/dump)
  DEST        local destination directory (default $HOME/backups/tennoworth)
  KEEP        local archives to retain, newest first (default 30)
  SSH, SCP    ssh and scp commands (default "ssh" and "scp"); options may be
              carried in the value, e.g. SSH='ssh -F /home/you/.ssh/config'
  SSH_OPTS, SCP_OPTS, COMMAND_TIMEOUT
              connection and whole-command bounds for unattended runs
  CORPUS_MATCH  extended regex a tar member must match to be a corpus log
              (default 'observations/sweep-[^/]*\.jsonl$')

Requires python3: the corpus run header is parsed as JSON, not pattern-matched.
USAGE
}

DRY_RUN=0
while [ $# -gt 0 ]; do
  case "$1" in
    --dry-run) DRY_RUN=1; shift;;
    -h|--help) usage; exit 0;;
    *) printf 'unknown argument: %s\n' "$1" >&2; exit 2;;
  esac
done

HOST="${HOST:-}"
VMID="${VMID:-}"
# Overridable because a session that cannot read the system ssh config must pass
# its own. Deliberately unquoted at the call sites so SSH can carry options:
# SSH='ssh -F /home/you/.ssh/config'.
SSH="${SSH:-ssh}"
SCP="${SCP:-scp}"
SSH_OPTS="${SSH_OPTS:--o BatchMode=yes -o ConnectTimeout=15 -o ServerAliveInterval=15 -o ServerAliveCountMax=4}"
SCP_OPTS="${SCP_OPTS:--o BatchMode=yes -o ConnectTimeout=15}"
COMMAND_TIMEOUT="${COMMAND_TIMEOUT:-600}"
DUMP_DIR="${DUMP_DIR:-/var/lib/vz/dump}"
DEST="${DEST:-$HOME/backups/tennoworth}"
KEEP="${KEEP:-30}"
CORPUS_MATCH="${CORPUS_MATCH:-observations/sweep-[^/]*\.jsonl$}"
# The observation format this backup must hold, kept in step with
# deploy/observations-check.sh through tests/fixtures/observation-retention.json;
# the deploy-layout test pins all three together.
FORMAT_SUPPORTED=1

MANIFEST="$DEST/manifest.jsonl"

say() { printf '%s\n' "$*"; }
die() { printf 'ABORT: %s\n' "$*" >&2; exit 1; }

json_str() {
  local s="$1"
  s="${s//\\/\\\\}"
  s="${s//\"/\\\"}"
  printf '"%s"' "$s"
}

# ---- inputs -----------------------------------------------------------------

# No default host or container: the node alias and the vmid are the deployment's
# facts, and guessing either would copy from the wrong machine or the wrong
# guest. An empty DEST would write the receipt somewhere unintended, and a KEEP
# of zero would delete the archive this run just proved.
[ -n "$HOST" ] || die "HOST is not set - the Proxmox node's ssh alias (HOST=pve)"
[ -n "$VMID" ] || die "VMID is not set - the container id to pull (VMID=110)"
case "$VMID" in
  *[!0-9]*) die "VMID must be numeric (got '$VMID')";;
esac
[ -n "$DEST" ] || die "DEST must not be empty"
case "$KEEP" in
  ''|*[!0-9]*) die "KEEP must be a positive integer (got '$KEEP')";;
esac
[ "$KEEP" -ge 1 ] || die "KEEP must be at least 1 - retaining none would delete the archive this run proves"

# A missing parser would make every archive unverifiable, and verification is the
# whole point of the run; refuse up front rather than discover it mid-transfer.
command -v python3 >/dev/null 2>&1 \
  || die "python3 is required to parse the corpus run header as JSON"

# ---- the newest dump on the node --------------------------------------------

# One ssh call lists every matching dump with its size and mtime. The names come
# back from the remote shell and are validated below before any of them is used
# as a path, so a broken or hostile listing cannot walk out of DUMP_DIR.
remote_dumps() {
  timeout "$COMMAND_TIMEOUT" $SSH $SSH_OPTS "$HOST" \
    "cd '$DUMP_DIR' 2>/dev/null || exit 1; for f in vzdump-lxc-$VMID-*.tar.zst; do [ -e \"\$f\" ] || continue; printf '%s %s %s\n' \"\$f\" \"\$(stat -c %s \"\$f\")\" \"\$(stat -c %Y \"\$f\")\"; done"
}

listing="$(remote_dumps)" || die "cannot list $DUMP_DIR on $HOST"
[ -n "$listing" ] || die "no vzdump-lxc-$VMID-*.tar.zst in $DUMP_DIR on $HOST"

# Newest by source mtime; the name breaks a tie, so two dumps in the same second
# still order deterministically instead of by whatever the node returns first.
newest="$(printf '%s\n' "$listing" | awk 'NF == 3' | sort -k3,3n -k1,1 | tail -1)"
ARCHIVE_NAME="$(printf '%s' "$newest" | awk '{print $1}')"
ARCHIVE_BYTES="$(printf '%s' "$newest" | awk '{print $2}')"
ARCHIVE_MTIME="$(printf '%s' "$newest" | awk '{print $3}')"
[ -n "$ARCHIVE_NAME" ] || die "could not read a dump name from $HOST:$DUMP_DIR"
case "$ARCHIVE_NAME" in
  "vzdump-lxc-$VMID-"*.tar.zst) ;;
  *) die "refusing unexpected dump name '$ARCHIVE_NAME' from $HOST:$DUMP_DIR";;
esac
case "$ARCHIVE_BYTES" in
  ''|*[!0-9]*) die "could not read the size of $ARCHIVE_NAME";;
esac
LOG_NAME="${ARCHIVE_NAME%.tar.zst}.log"

# ---- receipts already held --------------------------------------------------

# A receipt row whose recorded hash still matches the local copy means this exact
# archive was already proven. Re-downloading a multi-gigabyte dump to learn that
# again is wasted work and repeated wear on the node, so a re-run stops here -
# but only after the local bytes are re-hashed against the receipt, because a
# receipt row on its own does not prove the file is still intact.
manifest_field() { # <name> <key>
  local name="$1" key="$2" value
  [ -f "$MANIFEST" ] || return 1
  value="$(grep -F "\"name\":$(json_str "$name")," "$MANIFEST" 2>/dev/null \
    | head -1 \
    | grep -o "\"$key\":[^,}]*" \
    | head -1 \
    | cut -d: -f2- \
    | tr -d '"' || true)"
  [ -n "$value" ] || return 1
  printf '%s' "$value"
}

manifest_verified() { # <name> <bytes> <path>
  local name="$1" bytes="$2" path="$3" recorded_bytes recorded_sha got
  [ -f "$path" ] || return 1
  recorded_bytes="$(manifest_field "$name" bytes)" || return 1
  recorded_sha="$(manifest_field "$name" sha256)" || return 1
  [ "$recorded_bytes" = "$bytes" ] || return 1
  got="$(sha256sum "$path" | cut -d' ' -f1)" || return 1
  [ "$got" = "$recorded_sha" ]
}

# The manifest describes the archives actually held: a rotated archive is
# dropped with its file, so no row can ever name an artifact that is no longer
# here to restore. Rows are written through a temp file and renamed, so an
# interrupted rewrite leaves the previous complete receipt.
manifest_upsert() { # <name> <bytes> <sha256> <verified_at> <corpus_logs> <newest_corpus_log>
  local tmp="$MANIFEST.tmp"
  if [ -f "$MANIFEST" ]; then
    grep -vF "\"name\":$(json_str "$1")," "$MANIFEST" > "$tmp" || true
  else
    : > "$tmp"
  fi
  printf '{"name":%s,"bytes":%s,"sha256":%s,"verified_at":%s,"corpus_logs":%s,"newest_corpus_log":%s}\n' \
    "$(json_str "$1")" "$2" "$(json_str "$3")" "$(json_str "$4")" "$5" "$(json_str "$6")" >> "$tmp"
  mv "$tmp" "$MANIFEST"
}

manifest_drop() { # <name>
  [ -f "$MANIFEST" ] || return 0
  local tmp="$MANIFEST.tmp"
  grep -vF "\"name\":$(json_str "$1")," "$MANIFEST" > "$tmp" || true
  mv "$tmp" "$MANIFEST"
}

# ---- verification -----------------------------------------------------------

# The archive is only evidence if its bytes decompress, its tar listing is
# readable, and the newest corpus log inside it begins with the run header a
# replay needs. Each failure says which of the three failed, because "the backup
# is bad" without that is not actionable.
#
# The header fields are read with grep rather than a JSON parser on purpose:
# this runs on the maintainer's workstation with no jq/python assumption, and it
# is the same field-level shape deploy/observations-check.sh reads the live
# corpus by. A member is passed back to tar exactly as the listing spelled it,
# leading ./ included - GNU tar matches the stored name, not a normalised one.
# The first record has to be a complete run header: JSON that parses, with
# kind "run", the supported format, and a positive integer items. Reading it with
# a prefix test and a grep for `"items":` accepted a truncated line, a record
# with trailing text, and a wrong format - all of them things a replay cannot
# use, reported as a verified backup. The parser reads one line from stdin and
# prints the item count on success.
parse_run_header() { # <member>; first record on stdin
  python3 -c '
import json
import sys

member = sys.argv[1]
supported = int(sys.argv[2])
raw = sys.stdin.readline()
try:
    record = json.loads(raw)
except ValueError as exc:
    print(f"verify: the first record of {member} is not valid JSON: {exc}", file=sys.stderr)
    sys.exit(1)
if not isinstance(record, dict):
    print(f"verify: the first record of {member} is not a JSON object", file=sys.stderr)
    sys.exit(1)
kind = record.get("kind")
if kind != "run":
    print(f"verify: the first record of {member} has kind {kind!r}, not \"run\"", file=sys.stderr)
    sys.exit(1)
fmt = record.get("format")
# bool is an int subclass in Python, and `true` is not a format.
if isinstance(fmt, bool) or not isinstance(fmt, int) or fmt != supported:
    print(f"verify: the first record of {member} has format {fmt!r}, not the supported {supported}", file=sys.stderr)
    sys.exit(1)
items = record.get("items")
if isinstance(items, bool) or not isinstance(items, int) or items <= 0:
    print(f"verify: the first record of {member} reports items={items!r}, which is not a positive integer", file=sys.stderr)
    sys.exit(1)
print(items)
' "$1" "$FORMAT_SUPPORTED"
}

verify_archive() { # <path>; prints "<corpus_logs>|<newest_corpus_log>"
  local path="$1" listing members count newest_member first_line items
  if ! zstd -t "$path" >/dev/null 2>&1; then
    printf 'verify: zstd -t rejects %s - the copy is not an intact zstd stream\n' "$path" >&2
    return 1
  fi
  if ! listing="$(tar --zstd -tf "$path" 2>/dev/null)"; then
    printf 'verify: tar cannot list %s - the archive is not a readable tar stream\n' "$path" >&2
    return 1
  fi
  members="$(printf '%s\n' "$listing" | grep -E "$CORPUS_MATCH" | grep -v '/$' || true)"
  count="$(printf '%s' "$members" | grep -c . || true)"
  if [ "${count:-0}" -eq 0 ]; then
    printf 'verify: %s holds no corpus log matching %s\n' "$path" "$CORPUS_MATCH" >&2
    return 1
  fi
  # The log names carry their sweep's start stamp, so the lexicographic last is
  # the newest one.
  newest_member="$(printf '%s\n' "$members" | sort | tail -1)"
  # head closes the pipe after the first record, which can make tar exit on
  # SIGPIPE for a large log; the status is deliberately discarded and an empty
  # first line is the failure test instead.
  first_line="$(tar --zstd -xOf "$path" "$newest_member" 2>/dev/null | head -1 || true)"
  if [ -z "$first_line" ]; then
    printf 'verify: the newest corpus log %s has no first record to read\n' "$newest_member" >&2
    return 1
  fi
  if ! items="$(printf '%s\n' "$first_line" | parse_run_header "$newest_member")"; then
    return 1
  fi
  printf '%s|%s\n' "$count" "${newest_member#./}"
}

# ---- rotation ---------------------------------------------------------------

# Rotation runs only after a run in which verification passed. Newest first by
# mtime, the newest KEEP are kept, and everything older is removed with its log
# and its receipt row.
rotate_archives() {
  local names name index=0
  names="$(cd "$DEST" 2>/dev/null && ls -1t -- vzdump-lxc-"$VMID"-*.tar.zst 2>/dev/null || true)"
  [ -n "$names" ] || return 0
  while IFS= read -r name; do
    [ -n "$name" ] || continue
    index=$((index + 1))
    [ "$index" -le "$KEEP" ] && continue
    case "$name" in "$ARCHIVE_NAME") continue;; esac
    rm -f -- "$DEST/$name" "$DEST/${name%.tar.zst}.log"
    manifest_drop "$name"
    say "rotated out $name (beyond the newest $KEEP)"
  done <<< "$names"
}

if [ "$DRY_RUN" = 1 ]; then
  if manifest_verified "$ARCHIVE_NAME" "$ARCHIVE_BYTES" "$DEST/$ARCHIVE_NAME"; then
    say "dry run: $ARCHIVE_NAME is already verified locally; nothing would be downloaded"
  else
    say "dry run: would fetch $HOST:$DUMP_DIR/$ARCHIVE_NAME ($ARCHIVE_BYTES bytes, source mtime $ARCHIVE_MTIME, log $LOG_NAME)"
  fi
  say "dry run: would verify with zstd -t, tar --zstd -tf, and the newest corpus log's first record"
  say "dry run: destination $DEST, keeping the newest $KEEP archives"
  exit 0
fi

if manifest_verified "$ARCHIVE_NAME" "$ARCHIVE_BYTES" "$DEST/$ARCHIVE_NAME"; then
  say "$ARCHIVE_NAME is already verified locally; nothing to fetch"
  rotate_archives
  exit 0
fi

# ---- fetch ------------------------------------------------------------------

mkdir -p "$DEST"
part="$DEST/.$ARCHIVE_NAME.part"
log_part="$DEST/.$LOG_NAME.part"
# The .part is the only thing this run writes before verification; removing it on
# every exit path means a failed run leaves no half-copy to be mistaken for one.
cleanup_parts() { rm -f "$part" "$log_part"; }
trap cleanup_parts EXIT

say "fetching $ARCHIVE_NAME ($ARCHIVE_BYTES bytes)"
timeout "$COMMAND_TIMEOUT" $SCP $SCP_OPTS "$HOST:$DUMP_DIR/$ARCHIVE_NAME" "$part" \
  || die "could not fetch $HOST:$DUMP_DIR/$ARCHIVE_NAME"

verification=""
if ! verification="$(verify_archive "$part")"; then
  die "the fetched archive failed verification; no receipt was written and the previous archives are untouched"
fi
corpus_logs="$(printf '%s' "$verification" | cut -d'|' -f1)"
newest_corpus_log="$(printf '%s' "$verification" | cut -d'|' -f2)"
archive_bytes="$(stat -c %s "$part")"
archive_sha="$(sha256sum "$part" | cut -d' ' -f1)"

# Only a verified copy becomes the archive. Until this rename the previous one
# was still the newest proven file here, whatever happened to the transfer.
mv "$part" "$DEST/$ARCHIVE_NAME"

# The .log is the dump's own record and is copied for context. A missing sidecar
# is reported, not fatal: the archive is what preservation rests on, and failing
# a run over a diagnostic file would refuse a good backup.
if timeout "$COMMAND_TIMEOUT" $SCP $SCP_OPTS "$HOST:$DUMP_DIR/$LOG_NAME" "$log_part" 2>/dev/null; then
  mv "$log_part" "$DEST/$LOG_NAME"
else
  say "WARNING: no $LOG_NAME alongside $ARCHIVE_NAME on $HOST"
fi

verified_at="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
manifest_upsert "$ARCHIVE_NAME" "$archive_bytes" "$archive_sha" "$verified_at" "$corpus_logs" "$newest_corpus_log"
rotate_archives

say "verified $ARCHIVE_NAME: $archive_bytes bytes, sha256 $archive_sha, $corpus_logs corpus log(s), newest ${newest_corpus_log##*/}"
say "receipt: $MANIFEST"
