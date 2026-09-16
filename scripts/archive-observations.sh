#!/usr/bin/env bash
# Archive the box's per-item observation logs somewhere that is not the box.
#
# The corpus is the evidence for a later statistics-refresh decision, and the
# host keeps only a rolling window of it - retention on the box is sized for the
# evaluation, not for permanence. Nothing else copies these rows off the machine,
# so a disk loss or a bad prune would take the only record with it.
#
# The run is deliberately paranoid about what it stores: every file is hashed
# uncompressed on the box, transferred, hashed again here, then compressed and
# the decompressed bytes hashed a third time. A mismatch is a hard failure, not
# a warning - a silently corrupt archive is worse than no archive, because the
# replay built on it cannot tell.
#
# Read-only on the box. Destination is off the box by construction: the source is
# read over ssh and everything written here is local.
#
# Environment:
#   HOST         ssh target                      (default wfm)
#   SSH / SCP    ssh and scp commands            (default "ssh" and "scp")
#   SOURCE_DIR   observation directory on the box (default /srv/wfm/observations)
#   DEPLOYED     deployment record on the box     (default /srv/wfm/deployed.json)
#   DEST         local archive directory          (default
#                $HOME/.local/share/tennoworth/observations-archive)
set -euo pipefail

HOST="${HOST:-wfm}"
# Overridable because a session that cannot read the system ssh config must pass
# its own. Deliberately unquoted at the call sites so SSH can carry options:
# SSH='ssh -F /home/you/.ssh/config'.
SSH="${SSH:-ssh}"
SCP="${SCP:-scp}"
SOURCE_DIR="${SOURCE_DIR:-/srv/wfm/observations}"
DEPLOYED="${DEPLOYED:-/srv/wfm/deployed.json}"
DEST="${DEST:-$HOME/.local/share/tennoworth/observations-archive}"

MANIFEST="$DEST/manifest.jsonl"
STAGE="$DEST/.tmp"

say() { printf '%s\n' "$*"; }
die() { printf 'ABORT: %s\n' "$*" >&2; exit 1; }

json_str() {
  local s="$1"
  s="${s//\\/\\\\}"
  s="${s//\"/\\\"}"
  printf '"%s"' "$s"
}

# Only the published logs are archived. A `.partial` is still being appended to
# by a sweep that has not finished, so its bytes and hash are not a fact yet;
# the box keeps partials for post-mortem and the readiness check counts them.
# The name is checked before it is used as a path or embedded in the manifest.
remote_list() {
  $SSH "$HOST" "cd '$SOURCE_DIR' || exit 1; for f in sweep-*.jsonl; do [ -e \"\$f\" ] || continue; printf '%s %s %s\n' \"\$f\" \"\$(stat -c %s \"\$f\")\" \"\$(sha256sum \"\$f\" | cut -d' ' -f1)\"; done"
}

manifest_has() {
  [ -f "$MANIFEST" ] && grep -qF "\"name\":$(json_str "$1")," "$MANIFEST"
}

# Rewrite the manifest through a temp file and rename, so an interrupted append
# leaves the previous complete manifest rather than half a JSON line.
manifest_append() {
  local line
  line="$(printf '{"name":%s,"bytes":%s,"sha256":%s,"gz_sha256":%s,"archived_at":%s,"source_revision":%s}' \
    "$(json_str "$1")" "$2" "$(json_str "$3")" "$(json_str "$4")" "$(json_str "$5")" "$(json_str "$6")")"
  if [ -f "$MANIFEST" ]; then cat "$MANIFEST" > "$MANIFEST.tmp"; else : > "$MANIFEST.tmp"; fi
  printf '%s\n' "$line" >> "$MANIFEST.tmp"
  mv "$MANIFEST.tmp" "$MANIFEST"
}

mkdir -p "$DEST" "$STAGE"

# Provenance is not optional: an archive whose rows cannot be tied to the
# revision that wrote them cannot be replayed against that revision's code.
record="$($SSH "$HOST" "cat '$DEPLOYED'")" \
  || die "cannot read $DEPLOYED on $HOST - refusing to archive without provenance"
revision="$(printf '%s' "$record" | sed -n 's/.*"revision"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' | head -1)"
[ -n "$revision" ] || die "$DEPLOYED on $HOST has no revision field"

listing="$(remote_list)" || die "cannot list $SOURCE_DIR on $HOST"
[ -n "$listing" ] || die "no observation logs in $SOURCE_DIR on $HOST"

archived=0
skipped=0
failed=0

while read -r name size sha; do
  [ -n "$name" ] || continue
  case "$name" in
    */*|*..*) die "refusing remote name '$name'";;
  esac
  case "$sha" in
    [0-9a-f][0-9a-f]*) ;;
    *) die "the box returned no sha256 for $name";;
  esac
  if manifest_has "$name"; then
    skipped=$((skipped + 1))
    continue
  fi

  raw="$STAGE/$name"
  packed="$DEST/$name.gz"
  packed_tmp="$STAGE/$name.gz"
  rm -f "$raw" "$packed_tmp"
  say "archiving $name ($size bytes)"

  if ! $SCP -q "$HOST:$SOURCE_DIR/$name" "$raw"; then
    say "FAILED: $name did not transfer"
    rm -f "$raw"
    failed=$((failed + 1))
    continue
  fi

  # What the box said the bytes are, against what arrived.
  got_sha="$(sha256sum "$raw" | cut -d' ' -f1)"
  got_size="$(stat -c %s "$raw")"
  if [ "$got_sha" != "$sha" ] || [ "$got_size" != "$size" ]; then
    say "FAILED: $name does not match the box (sha256 $got_sha vs $sha, size $got_size vs $size)"
    rm -f "$raw"
    failed=$((failed + 1))
    continue
  fi

  gzip -9 -c "$raw" > "$packed_tmp"
  # A compress step that quietly damaged the stream would otherwise be recorded
  # as verified against the pre-compression hash.
  packed_sha="$(gzip -dc "$packed_tmp" | sha256sum | cut -d' ' -f1)"
  gz_sha="$(sha256sum "$packed_tmp" | cut -d' ' -f1)"
  if [ "$packed_sha" != "$sha" ]; then
    say "FAILED: $name does not decompress back to the box's content"
    rm -f "$raw" "$packed_tmp"
    failed=$((failed + 1))
    continue
  fi

  mv "$packed_tmp" "$packed"
  rm -f "$raw"
  manifest_append "$name" "$got_size" "$got_sha" "$gz_sha" "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$revision"
  archived=$((archived + 1))
done <<< "$listing"

say "archive: $archived new, $skipped already present, $failed failed -> $DEST (revision $revision)"
[ "$failed" = 0 ] || die "$failed file(s) failed verification; the archive is incomplete"
