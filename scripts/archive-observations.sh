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
# A run that succeeds publishes a receipt next to the archive: a header naming
# the source and the verification time, then one line per covered file. That
# receipt is the heartbeat the box can read, so an archive that quietly stopped
# running is visible from the box instead of only from this host.
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
RECEIPT="$DEST/receipt.jsonl"
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

manifest_field() { # <name> <key>
  grep -F "\"name\":$(json_str "$1")," "$MANIFEST" 2>/dev/null \
    | head -1 \
    | grep -o "\"$2\":[^,}]*" \
    | head -1 \
    | cut -d: -f2- \
    | tr -d '"' || true
}

# Rewrite the manifest through a temp file and rename, so an interrupted write
# leaves the previous complete manifest rather than half a JSON line. A repair
# replaces the existing row: one row per file, always.
manifest_upsert() {
  local line
  line="$(printf '{"name":%s,"bytes":%s,"sha256":%s,"gz_sha256":%s,"archived_at":%s,"deployed_revision_at_archive":%s,"producer_revision":null}' \
    "$(json_str "$1")" "$2" "$(json_str "$3")" "$(json_str "$4")" "$(json_str "$5")" "$(json_str "$6")")"
  if [ -f "$MANIFEST" ]; then
    grep -vF "\"name\":$(json_str "$1")," "$MANIFEST" > "$MANIFEST.tmp" || true
  else
    : > "$MANIFEST.tmp"
  fi
  printf '%s\n' "$line" >> "$MANIFEST.tmp"
  mv "$MANIFEST.tmp" "$MANIFEST"
}

# A file counts as archived only when the stored artifact is present AND still
# matches both recorded hashes AND still describes what the box holds now. A
# manifest row on its own proves nothing: deleting the .gz used to leave every
# later run reporting success over an archive that no longer existed.
stored_ok() { # <name> <box-sha256>
  local name="$1" sha="$2" stored recorded_gz recorded_sha
  [ -f "$MANIFEST" ] || return 1
  [ -f "$DEST/$name.gz" ] || return 1
  recorded_sha="$(manifest_field "$name" sha256)"
  recorded_gz="$(manifest_field "$name" gz_sha256)"
  [ "$recorded_sha" = "$sha" ] || return 1
  [ -n "$recorded_gz" ] || return 1
  stored="$(sha256sum "$DEST/$name.gz" | cut -d' ' -f1)"
  [ "$stored" = "$recorded_gz" ] || return 1
  [ "$(gzip -dc "$DEST/$name.gz" | sha256sum | cut -d' ' -f1)" = "$recorded_sha" ] || return 1
}

# The receipt is what the box reads, so it carries the source identity and the
# verification instant as well as the covered rows. Written through a temp file
# and renamed, and only after a run in which nothing failed.
publish_receipt() {
  local files
  files="$(grep -c '"name":' "$MANIFEST" 2>/dev/null || true)"
  {
    printf '{"kind":"archive_receipt","format":1,"verified_at":%s,"archive_host":%s,"source_host":%s,"source_dir":%s,"deployed_revision_at_archive":%s,"files":%s}\n' \
      "$(json_str "$(date -u +%Y-%m-%dT%H:%M:%SZ)")" \
      "$(json_str "$(hostname 2>/dev/null || printf unknown)")" \
      "$(json_str "$HOST")" "$(json_str "$SOURCE_DIR")" "$(json_str "$revision")" "${files:-0}"
    if [ -f "$MANIFEST" ]; then cat "$MANIFEST"; fi
  } > "$STAGE/receipt.jsonl"
  mv "$STAGE/receipt.jsonl" "$RECEIPT"
  say "receipt: $RECEIPT (${files:-0} files)"
}

mkdir -p "$DEST" "$STAGE"

# Provenance is not optional: an archive whose rows cannot be tied to the
# deployment that was live when they were collected cannot be replayed against
# that revision's code. This is the deployment *observed at archive time* - it
# is not a claim about which revision produced each file, which nothing in the
# log header records, so that field stays explicitly null.
record="$($SSH "$HOST" "cat '$DEPLOYED'")" \
  || die "cannot read $DEPLOYED on $HOST - refusing to archive without provenance"
revision="$(printf '%s' "$record" | sed -n 's/.*"revision"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' | head -1)"
[ -n "$revision" ] || die "$DEPLOYED on $HOST has no revision field"

listing="$(remote_list)" || die "cannot list $SOURCE_DIR on $HOST"
[ -n "$listing" ] || die "no observation logs in $SOURCE_DIR on $HOST"

archived=0
skipped=0
repaired=0
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
  if stored_ok "$name" "$sha"; then
    skipped=$((skipped + 1))
    continue
  fi

  raw="$STAGE/$name"
  packed="$DEST/$name.gz"
  packed_tmp="$STAGE/$name.gz"
  rm -f "$raw" "$packed_tmp"
  repairing=0
  if [ -f "$MANIFEST" ] && grep -qF "\"name\":$(json_str "$name")," "$MANIFEST"; then
    say "repairing $name (stored copy is missing or does not match the box)"
    repairing=1
  else
    say "archiving $name ($size bytes)"
  fi

  if ! $SCP -q "$HOST:$SOURCE_DIR/$name" "$raw"; then
    say "FAILED: $name did not transfer"
    rm -f "$raw"
    failed=$((failed + 1))
    continue
  fi

  # What the box said the bytes are, against what arrived. A hashing failure is
  # reported as the mismatch it is, not as an abort that names nothing.
  got_sha="$(sha256sum "$raw" | cut -d' ' -f1 || true)"
  got_size="$(stat -c %s "$raw" || true)"
  if [ "$got_sha" != "$sha" ] || [ "$got_size" != "$size" ]; then
    say "FAILED: $name does not match the box (sha256 $got_sha vs $sha, size $got_size vs $size)"
    rm -f "$raw"
    failed=$((failed + 1))
    continue
  fi

  gzip -9 -c "$raw" > "$packed_tmp"
  # A compress step that quietly damaged the stream would otherwise be recorded
  # as verified against the pre-compression hash. A failed decompress is that
  # same failure, so it is compared rather than allowed to abort the run.
  packed_sha="$(gzip -dc "$packed_tmp" | sha256sum | cut -d' ' -f1 || true)"
  gz_sha="$(sha256sum "$packed_tmp" | cut -d' ' -f1 || true)"
  if [ "$packed_sha" != "$sha" ]; then
    say "FAILED: $name does not decompress back to the box's content"
    rm -f "$raw" "$packed_tmp"
    failed=$((failed + 1))
    continue
  fi

  mv "$packed_tmp" "$packed"
  rm -f "$raw"
  manifest_upsert "$name" "$got_size" "$got_sha" "$gz_sha" "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$revision"
  if [ "$repairing" = 1 ]; then
    repaired=$((repaired + 1))
  else
    archived=$((archived + 1))
  fi
done <<< "$listing"

# A failed run does not publish: the receipt is the last-success heartbeat, and
# refreshing it over an incomplete archive would be the silent success this
# script exists to refuse.
if [ "$failed" != 0 ]; then
  say "archive: $archived new, $repaired repaired, $skipped intact, $failed failed -> $DEST"
  die "$failed file(s) failed verification; the archive is incomplete and no receipt was published"
fi

publish_receipt
say "archive: $archived new, $repaired repaired, $skipped intact, $failed failed -> $DEST (deployed $revision)"
