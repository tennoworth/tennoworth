#!/usr/bin/env bash
# Daily readiness check for the observation corpus.
#
# The corpus is the evidence a later decision about statistics-refresh schedules
# will rest on. A corpus that has quietly lost sweeps, kept a truncated log,
# dropped the rows a rebuild would need, or outlived its retention is not
# evidence - and a check that cannot say so is worse than none, because it makes
# an unsound corpus look signed off. Every check here is therefore written to
# fail closed: missing evidence is an error, not an empty result.
#
# The same rule applies to preservation. A corpus that is sound but not being
# preserved is one disk failure away from being no corpus at all, so the check
# has to be told where preservation lives and must never imply it can see what it
# cannot. In `on-box-archive` mode the receipt the archive host publishes is
# required evidence: absent, stale, disagreeing with the box, or missing a
# completed file past its archival deadline are all failures, and an unmonitored
# archive can never render as ready. In `external-backup` mode preservation is
# the host-level Proxmox backup of this whole container, verified by an
# operator-run pull-and-verify job on another machine; no receipt is required,
# nothing here is read as proof, and the report says so in as many words. A mode
# that is unset or unrecognised is not ready - an undeclared claim is not a
# satisfied one.
#
# Scope: this answers "is the corpus sound and complete, and is its preservation
# declared honestly". Whether a proposed refresh schedule meets its acceptance
# thresholds is a different question with a different owner (`wfm-scrape
# replay`); nothing here may answer it, or a schedule could be declared good
# because the collection ran.
#
# Runs on the box from wfm-observations-check.timer, and locally against any
# directory through --observations/--out.
#
# Exit status: 0 ready, 1 not ready (a report is still written), 2 the check
# could not run at all.
set -euo pipefail

usage() {
  cat <<'USAGE'
usage: observations-check.sh [options]

  --observations <dir>        observation log directory (default /srv/wfm/observations)
  --out <json>                write the report here (always also printed to stdout)
  --csv <path>                the sweep's CSV (default /srv/wfm/app/wfm_results.csv)
  --snapshot <path>           published snapshot (default /srv/wfm/app/frontend/public/market.json)
  --deployed <path>           deployment record (default /srv/wfm/deployed.json)
  --binary <path>             installed scraper (default /srv/wfm/bin/wfm-scrape)
  --archive-receipt <path>    receipt published by the archive host (default
                              /srv/wfm/data/observations-check/archive-receipt.jsonl)
  --preservation <mode>       where the corpus is preserved: external-backup (a
                              host-level Proxmox backup of this container,
                              verified by an operator-run job elsewhere) or
                              on-box-archive (the archive host receipt above).
                              Defaults to $OBSERVATIONS_PRESERVATION; an unset or
                              unrecognised mode is not ready.
  --archive-deadline-seconds <n>  age at which an unarchived log or an unrefreshed
                              receipt is late (default 129600 = a daily run plus
                              half a day of grace)
  --pair-tolerance-seconds <n>    how far a sweep's journal start may sit from the
                              log header it ran (default 300)
  --interval-seconds <n>      scheduled sweep spacing (default 7200)
  --retention-bytes <n>       observation cap (default 2147483648)
  --retention-age-days <n>    observation age limit (default 56)
  --disk-floor-bytes <n>      free-space floor (default 1073741824)
  --now <iso>                 evaluation instant, for reproducing a report
USAGE
}

OBSERVATIONS="/srv/wfm/observations"
OUT=""
CSV="/srv/wfm/app/wfm_results.csv"
SNAPSHOT="/srv/wfm/app/frontend/public/market.json"
DEPLOYED="/srv/wfm/deployed.json"
BINARY="/srv/wfm/bin/wfm-scrape"
ARCHIVE_RECEIPT="/srv/wfm/data/observations-check/archive-receipt.jsonl"
# Where preservation lives. Named explicitly by the deployment - there is no
# permissive default, because a check that guesses would report a claim nobody
# made.
PRESERVATION="${OBSERVATIONS_PRESERVATION:-}"
ARCHIVE_DEADLINE_SECONDS=$((36 * 60 * 60))
# Clocks are not perfectly synchronised, so a receipt may be a little ahead of
# this host; anything further ahead is not freshness.
ARCHIVE_FUTURE_SKEW_SECONDS=600
PAIR_TOLERANCE_SECONDS=300
INTERVAL_SECONDS=7200
# Kept in step with rust/wfm-scrape/src/observations.rs through
# tests/fixtures/observation-retention.json; the deploy-layout test pins all
# three together so the box cannot judge the corpus by numbers the pipeline no
# longer prunes with.
RETENTION_BYTES=2147483648
RETENTION_AGE_DAYS=56
PARTIAL_MAX_AGE_SECONDS=10800
DISK_FLOOR_BYTES=1073741824
WARN_ELAPSED_SECONDS=5400
URGENT_ELAPSED_SECONDS=6300
BOUNDARY_GRACE_SECONDS=900
FORMAT_SUPPORTED=1
NOW=""

while [ $# -gt 0 ]; do
  case "$1" in
    --observations) OBSERVATIONS="$2"; shift 2;;
    --out) OUT="$2"; shift 2;;
    --csv) CSV="$2"; shift 2;;
    --snapshot) SNAPSHOT="$2"; shift 2;;
    --deployed) DEPLOYED="$2"; shift 2;;
    --binary) BINARY="$2"; shift 2;;
    --archive-receipt) ARCHIVE_RECEIPT="$2"; shift 2;;
    --preservation) PRESERVATION="$2"; shift 2;;
    --archive-deadline-seconds) ARCHIVE_DEADLINE_SECONDS="$2"; shift 2;;
    --pair-tolerance-seconds) PAIR_TOLERANCE_SECONDS="$2"; shift 2;;
    --interval-seconds) INTERVAL_SECONDS="$2"; shift 2;;
    --retention-bytes) RETENTION_BYTES="$2"; shift 2;;
    --retention-age-days) RETENTION_AGE_DAYS="$2"; shift 2;;
    --disk-floor-bytes) DISK_FLOOR_BYTES="$2"; shift 2;;
    --now) NOW="$2"; shift 2;;
    -h|--help) usage; exit 0;;
    *) printf 'unknown argument: %s\n' "$1" >&2; exit 2;;
  esac
done

ERRORS=()
WARNINGS=()
error() { ERRORS+=("$1"); }
warn() { WARNINGS+=("$1"); }

# Validated after error() exists, so an undeclared or unrecognised mode lands in
# the report as a reason instead of ending the run before one is written.
case "$PRESERVATION" in
  external-backup|on-box-archive) ;;
  '') error "preservation mode not declared - set --preservation or OBSERVATIONS_PRESERVATION";;
  *) error "unrecognised preservation mode '$PRESERVATION' - expected external-backup or on-box-archive";;
esac

jstr() {
  local s="$1"
  s="${s//\\/\\\\}"
  s="${s//\"/\\\"}"
  printf '"%s"' "$s"
}

jarr() {
  local out="[" first=1 x
  for x in "$@"; do
    [ "$first" = 1 ] || out+=","
    first=0
    out+="$(jstr "$x")"
  done
  printf '%s]' "$out"
}

jstr_or_null() {
  if [ -n "${1:-}" ]; then jstr "$1"; else printf 'null'; fi
}

# A JSON number, or null where the datum is genuinely absent. Deliberately never
# a default: a missing measurement that renders as 0 reads as a healthy one.
jnum() {
  case "${1:-}" in
    ''|n/a|unknown) printf 'null';;
    *) printf '%s' "$1";;
  esac
}

iso_to_epoch() { date -u -d "$1" +%s 2>/dev/null; }
epoch_to_iso() { date -u -d "@$1" +%Y-%m-%dT%H:%M:%SZ; }
mtime_of() { stat -c %Y "$1" 2>/dev/null || printf '0'; }
now_epoch() { date -u +%s; }

# The log filename carries the sweep's start stamp, which is the fallback when a
# header is too damaged to read one.
name_stamp() {
  printf '%s' "$1" \
    | sed -E 's/^sweep-([0-9]{4}-[0-9]{2}-[0-9]{2})T([0-9]{2})-([0-9]{2})-([0-9]{2})Z.*$/\1T\2:\3:\4Z/'
}

# One record per line and the tag first, because that is how the writer emits
# them; anything else is a log this check cannot read as evidence. The
# whitespace tolerance is for /srv/wfm/deployed.json, which is pretty-printed.
#
# Absence is a result, not a failure: with `set -o pipefail` a grep that finds
# no match would abort the run before any report existed, which is the one
# outcome a fail-closed check must never produce - a missing field has to be
# visible in the report as absent.
record_field() {
  local value
  value="$(printf '%s' "$1" \
    | grep -o "\"$2\"[[:space:]]*:[[:space:]]*[^,}]*" 2>/dev/null \
    | head -1 \
    | sed 's/^[^:]*:[[:space:]]*//' \
    | tr -d '"')" || true
  printf '%s' "$value"
}

if [ -n "$NOW" ]; then
  NOW_EPOCH="$(iso_to_epoch "$NOW")" || true
  case "${NOW_EPOCH:-}" in
    ''|*[!0-9]*) printf 'cannot parse --now: %s\n' "$NOW" >&2; exit 2;;
  esac
else
  NOW_EPOCH="$(now_epoch)"
fi
NOW_ISO="$(epoch_to_iso "$NOW_EPOCH")"
RETENTION_AGE_SECONDS=$((RETENTION_AGE_DAYS * 24 * 60 * 60))

# ---- the corpus -------------------------------------------------------------

valid=0
partials=0
malformed=0
missing_summary=0
unsupported=0
total_bytes=0
oldest_start=""
oldest_log_age=""
newest_start=""
newest_mtime=0
newest_name=""
oldest_partial_age=""
completeness_failures=()
day_counts="$(mktemp)"
starts_file="$(mktemp)"
# One row per log with a readable header: start|name|items|kept. The service
# section pairs a journal invocation back to the sweep it ran by these stamps.
log_rows="$(mktemp)"
# Completed logs only: name|mtime, so the archive rules can ask which of them
# have waited past the archival deadline.
completed_rows="$(mktemp)"
trap 'rm -f "$day_counts" "$starts_file" "$log_rows" "$completed_rows"' EXIT

if [ ! -d "$OBSERVATIONS" ]; then
  error "observations directory $OBSERVATIONS does not exist"
else
  found=0
  for path in "$OBSERVATIONS"/sweep-*; do
    [ -e "$path" ] || continue
    found=1
    base="$(basename "$path")"
    # A file that cannot be sized or read is an error in the report, not an
    # arithmetic failure that ends the run before one exists.
    size="$(stat -c %s "$path" 2>/dev/null || true)"
    if [ -z "$size" ]; then
      error "$base cannot be read"
      continue
    fi
    total_bytes=$((total_bytes + size))

    case "$base" in
      *.partial)
        partials=$((partials + 1))
        age=$((NOW_EPOCH - $(mtime_of "$path")))
        if [ -z "$oldest_partial_age" ] || [ "$age" -gt "$oldest_partial_age" ]; then
          oldest_partial_age="$age"
        fi
        continue
        ;;
    esac

    total_lines="$(wc -l < "$path" 2>/dev/null || true)"
    items="$(grep -c '"kind":"item"' "$path" || true)"
    kept="$(grep -c '"outcome":"kept"' "$path" || true)"
    rejected="$(grep -c '"outcome":"below_volume"\|"outcome":"no_statistics"' "$path" || true)"
    books="$(grep -c '"book":{' "$path" || true)"
    unique_slugs="$(grep -o '"slug":"[^"]*"' "$path" | sort -u | wc -l || true)"
    runs="$(grep -c '^{"kind":"run"' "$path" || true)"
    summaries="$(grep -c '^{"kind":"summary"' "$path" || true)"
    unreadable="$(grep -vc '^{"kind":"\(run\|item\|summary\)"' "$path" || true)"

    if [ -z "$total_lines" ] || [ "$unreadable" != 0 ] || [ "$total_lines" = 0 ]; then
      malformed=$((malformed + 1))
      error "$base is malformed ($unreadable line(s) are not readable records)"
      continue
    fi

    header="$(head -1 "$path" 2>/dev/null || true)"
    footer="$(tail -1 "$path" 2>/dev/null || true)"
    if [ "$runs" != 1 ] || ! printf '%s' "$header" | grep -q '^{"kind":"run"'; then
      malformed=$((malformed + 1))
      error "$base does not begin with exactly one run header"
      continue
    fi

    format="$(record_field "$header" format)"
    if [ "${format:-}" != "$FORMAT_SUPPORTED" ]; then
      unsupported=$((unsupported + 1))
      error "$base has unsupported observation format '${format:-none}'"
      continue
    fi

    started="$(record_field "$header" started_at)"
    [ -n "$started" ] || started="$(name_stamp "$base")"
    run_items="$(record_field "$header" items)"
    start_epoch="$(iso_to_epoch "$started")" || true
    if [ -z "${start_epoch:-}" ]; then
      error "$base has an unreadable start stamp '$started'"
      continue
    fi

    if [ "$summaries" != 1 ] || ! printf '%s' "$footer" | grep -q '^{"kind":"summary"'; then
      missing_summary=$((missing_summary + 1))
      error "$base has no summary - the sweep did not complete"
      printf '%s|%s|%s|\n' "$start_epoch" "$base" "${run_items:-}" >> "$log_rows"
      continue
    fi

    scanned="$(record_field "$footer" scanned)"
    sum_kept="$(record_field "$footer" kept)"

    log_failures=()
    [ "${items:-0}" = "${run_items:-x}" ] || log_failures+=("$items item rows against run.items=$run_items")
    [ "${items:-0}" = "${scanned:-x}" ] || log_failures+=("$items item rows against summary.scanned=$scanned")
    [ "${kept:-0}" = "${sum_kept:-x}" ] || log_failures+=("$kept kept rows against summary.kept=$sum_kept")
    [ "$((kept + rejected))" = "${items:-0}" ] || log_failures+=("kept+rejected=$((kept + rejected)) against $items rows")
    [ "${unique_slugs:-0}" = "${items:-0}" ] || log_failures+=("$unique_slugs unique slugs against $items rows")
    # The rows a rebuild needs are the kept items and the books the sweep
    # fetched for them; either going missing makes the log unrunnable evidence.
    [ "${books:-0}" = "${kept:-0}" ] || log_failures+=("$books books against $kept kept rows")
    if [ "${items:-0}" -gt 0 ] && [ "${kept:-0}" -eq "${items:-0}" ] && [ "${run_items:-0}" -gt "${items:-0}" ]; then
      log_failures+=("no rejected items retained")
    fi
    if [ "${#log_failures[@]}" -gt 0 ]; then
      completeness_failures+=("$base: ${log_failures[*]}")
      error "$base is incomplete: ${log_failures[*]}"
    fi

    valid=$((valid + 1))
    printf '%s\n' "$started" >> "$starts_file"
    printf '%s\n' "${started%%T*}" >> "$day_counts"
    printf '%s|%s|%s|%s\n' "$start_epoch" "$base" "${run_items:-}" "${sum_kept:-}" >> "$log_rows"
    printf '%s|%s\n' "$base" "$(mtime_of "$path")" >> "$completed_rows"

    if [ -z "$oldest_start" ] || [ "$start_epoch" -lt "$(iso_to_epoch "$oldest_start")" ]; then
      oldest_start="$started"
    fi
    if [ -z "$newest_start" ] || [ "$start_epoch" -gt "$(iso_to_epoch "$newest_start")" ]; then
      newest_start="$started"
      newest_name="$base"
    fi
    mtime="$(mtime_of "$path")"
    if [ "$mtime" -gt "$newest_mtime" ]; then
      newest_mtime="$mtime"
    fi
    if [ -z "$oldest_log_age" ] || [ "$((NOW_EPOCH - mtime))" -gt "$oldest_log_age" ]; then
      oldest_log_age=$((NOW_EPOCH - mtime))
    fi
  done
  [ "$found" = 1 ] || error "no observation logs in $OBSERVATIONS"
fi

if [ "$partials" != 0 ] && [ -n "$oldest_partial_age" ] && [ "$oldest_partial_age" -gt "$PARTIAL_MAX_AGE_SECONDS" ]; then
  error "a partial observation log is $((oldest_partial_age / 60)) minutes old - a sweep is stuck"
fi

# ---- the evaluation window --------------------------------------------------

window_span=0
complete_four_weeks=false
if [ -n "$oldest_start" ] && [ -n "$newest_start" ]; then
  window_span=$(( $(iso_to_epoch "$newest_start") - $(iso_to_epoch "$oldest_start") ))
  [ "$window_span" -ge $((28 * 24 * 60 * 60)) ] && complete_four_weeks=true
fi

# A sweep is missing when the step between consecutive logs is a whole number of
# schedule slots greater than one. Counting slots from the window length instead
# would drift: the timer's jitter and the sweep's own runtime make consecutive
# starts more than the nominal interval apart, and rounding that up would invent
# a missing sweep on a perfectly healthy corpus.
missing=0
longest_gap=0
if [ -s "$starts_file" ]; then
  previous=""
  while read -r started; do
    [ -n "$started" ] || continue
    current="$(iso_to_epoch "$started")"
    if [ -n "$previous" ]; then
      gap=$((current - previous))
      [ "$gap" -gt "$longest_gap" ] && longest_gap="$gap"
      slots=$(( (gap + INTERVAL_SECONDS / 2) / INTERVAL_SECONDS ))
      [ "$slots" -lt 1 ] && slots=1
      missing=$((missing + slots - 1))
    fi
    previous="$current"
  done < <(sort "$starts_file")
fi
expected=$((valid + missing))
if [ "$missing" -gt 0 ]; then
  error "$missing scheduled sweep(s) have no log (expected $expected, found $valid)"
fi

per_day="$(sort "$day_counts" | uniq -c | awk '{printf "%s\"%s\":%s", (NR>1?",":""), $2, $1}')"

newest_age=""
if [ "$newest_mtime" != 0 ]; then
  newest_age=$((NOW_EPOCH - newest_mtime))
fi

snapshot_age=""
snapshot_state="missing"
if [ -f "$SNAPSHOT" ]; then
  snapshot_state="present"
  snapshot_age=$((NOW_EPOCH - $(mtime_of "$SNAPSHOT")))
else
  error "published snapshot $SNAPSHOT is missing"
fi
if [ "$snapshot_state" = present ] && [ "$newest_mtime" != 0 ] && [ "$(mtime_of "$SNAPSHOT")" -lt "$newest_mtime" ]; then
  error "the published snapshot predates the newest completed sweep"
fi

# ---- CSV agreement ----------------------------------------------------------
# Only the newest sweep's CSV is still on disk; the file is overwritten every
# sweep, so an older log's row count is simply not observable and is reported as
# unchecked rather than assumed to agree.
csv_state="missing"
csv_rows=""
csv_sweep="$newest_name"
if [ -n "$newest_name" ]; then
  if [ -f "$CSV" ]; then
    csv_state="checked"
    csv_rows="$(wc -l < "$CSV" 2>/dev/null || true)"
    if [ -z "$csv_rows" ]; then
      csv_state="unreadable"
      error "cannot read $CSV"
      csv_rows=""
    else
      csv_rows=$((csv_rows - 1))
      kept_newest="$(grep -c '"outcome":"kept"' "$OBSERVATIONS/$newest_name" || true)"
      if [ "$csv_rows" != "$kept_newest" ]; then
        error "$newest_name kept $kept_newest rows but $CSV holds $csv_rows"
      fi
    fi
  else
    error "no CSV at $CSV to compare the newest sweep against"
  fi
fi

# ---- retention and disk -----------------------------------------------------

if [ "$total_bytes" -gt "$RETENTION_BYTES" ]; then
  error "observation logs use $total_bytes bytes, over the $RETENTION_BYTES cap"
fi
oldest_age_display="$(jnum "${oldest_log_age:-}")"
if [ -n "$oldest_log_age" ] && [ "$oldest_log_age" -gt "$RETENTION_AGE_SECONDS" ]; then
  error "the oldest observation log is $((oldest_log_age / 86400)) days old, past the $RETENTION_AGE_DAYS-day limit"
fi

disk_path="$OBSERVATIONS"
[ -d "$disk_path" ] || disk_path="$(dirname "$OBSERVATIONS")"
free_bytes="$(df -P -B1 "$disk_path" 2>/dev/null | awk 'NR==2 {print $4}' || true)"
if [ -z "${free_bytes:-}" ]; then
  error "cannot read free space on $disk_path"
elif [ "$free_bytes" -lt "$DISK_FLOOR_BYTES" ]; then
  error "only $free_bytes bytes free on $disk_path, under the $DISK_FLOOR_BYTES floor"
fi

# ---- archive (on-box receipt) -----------------------------------------------
# The receipt is the archive host's heartbeat, and only the on-box archive
# deployment has one. A missing receipt there means archival is unmonitored,
# which is a failure in its own right: the whole point of the archive is that
# this corpus survives something happening to the box. In external-backup mode
# this evidence does not exist on this host at all, so the block is skipped
# rather than rendered as a missing file - the preservation report below carries
# the honest statement instead.

archive_status="not-applicable"
archive_verified=""
archive_age=""
archive_covered=""
archive_receipt_revision=""
archive_producer_unknown=0
overdue=()
mismatched=()

if [ "$PRESERVATION" = on-box-archive ]; then
  archive_status="missing"

  if [ ! -f "$ARCHIVE_RECEIPT" ]; then
    error "no archive receipt at $ARCHIVE_RECEIPT - the corpus is not being preserved"
  else
    # An unreadable receipt is still a verdict: without the guard, an I/O or
    # permission error here aborts the run before any report is written, which is
    # the outcome this check exists to make impossible.
    receipt_header=""
    if ! receipt_header="$(head -1 "$ARCHIVE_RECEIPT" 2>/dev/null)"; then
      receipt_header=""
    fi
    if [ -z "$receipt_header" ]; then
      archive_status="unreadable"
      error "cannot read the archive receipt at $ARCHIVE_RECEIPT"
    elif ! printf '%s' "$receipt_header" | grep -q '"kind":"archive_receipt"'; then
      archive_status="unreadable"
      error "$ARCHIVE_RECEIPT is not an archive receipt"
    else
      archive_status="ok"
      archive_verified="$(record_field "$receipt_header" verified_at)"
      archive_receipt_revision="$(record_field "$receipt_header" deployed_revision_at_archive)"
      archive_covered="$(grep -c '"name":' "$ARCHIVE_RECEIPT" || true)"
      archive_producer_unknown="$(grep -c '"producer_revision":null' "$ARCHIVE_RECEIPT" || true)"

      verified_epoch=""
      if [ -n "$archive_verified" ]; then
        verified_epoch="$(iso_to_epoch "$archive_verified")" || true
      fi
      if [ -z "${verified_epoch:-}" ]; then
        archive_status="unreadable"
        error "the archive receipt has no readable verified_at"
      else
        archive_age=$((NOW_EPOCH - verified_epoch))
        if [ "$archive_age" -gt "$ARCHIVE_DEADLINE_SECONDS" ]; then
          archive_status="stale"
          error "the archive receipt is $((archive_age / 3600)) hours old - the archive job has not succeeded"
        elif [ "$archive_age" -lt $((0 - ARCHIVE_FUTURE_SKEW_SECONDS)) ]; then
          # A claim dated in the future is not freshness; it is a broken clock or a
          # fabricated receipt, and either way it must not read as current.
          archive_status="future"
          error "the archive receipt is dated $((0 - archive_age)) seconds in the future"
        fi
      fi

      # A receipt that names a file the box no longer has is not a failure (the
      # box prunes), but one that disagrees about a file still here means the
      # archived copy is not this file.
      while read -r line; do
        [ -n "$line" ] || continue
        name=""
        sha=""
        if [[ "$line" =~ \"name\":\"([^\"]*)\" ]]; then name="${BASH_REMATCH[1]}"; fi
        if [[ "$line" =~ \"sha256\":\"([0-9a-f]*)\" ]]; then sha="${BASH_REMATCH[1]}"; fi
        [ -n "$name" ] || continue
        [ -f "$OBSERVATIONS/$name" ] || continue
        if [ -n "$sha" ]; then
          got="$(sha256sum "$OBSERVATIONS/$name" 2>/dev/null | cut -d' ' -f1 || true)"
          [ "$got" = "$sha" ] || mismatched+=("$name")
        fi
      done < <(grep '"name":' "$ARCHIVE_RECEIPT" || true)
      if [ "${#mismatched[@]}" -gt 0 ]; then
        error "the archived copy of ${#mismatched[@]} log(s) does not match the box: ${mismatched[*]}"
      fi

      # A daily archive plus half a day of grace: a log renamed moments before the
      # run waits for the next one, and anything older than that has been missed.
      while IFS='|' read -r name mtime; do
        [ -n "$name" ] || continue
        [ "$((NOW_EPOCH - mtime))" -gt "$ARCHIVE_DEADLINE_SECONDS" ] || continue
        grep -qF "\"name\":\"$name\"" "$ARCHIVE_RECEIPT" || overdue+=("$name")
      done < "$completed_rows"
      if [ "${#overdue[@]}" -gt 0 ]; then
        archive_status="lagging"
        error "${#overdue[@]} completed log(s) are past their archival deadline: ${overdue[*]}"
      fi
    fi
  fi
fi

# ---- service health ---------------------------------------------------------
# The `sweep metrics:` line is the sweep's own record of what it asked WFM for.
# Its `elapsed_ms` is the sum of per-request latencies across workers - on a
# healthy two-worker sweep it is about twice the wall clock - so the elapsed
# thresholds gate on the wall clock of the same systemd invocation, and the
# metrics line is quoted alongside for context rather than used as the clock.
#
# The invocation id is what ties the metrics line, the start and the end
# together: matching on "the nearest Starting line" can pair one sweep's start
# with another's end, and it cannot tell a missing start from a complete
# invocation. A start that cannot be established for the same invocation is an
# explicit failure, never a silently skipped gate.
JOURNALCTL="${JOURNALCTL:-journalctl}"
SYSTEMCTL="${SYSTEMCTL:-systemctl}"

service_state=""
service_result=""
metrics_found=false
metrics_line=""
invocation=""
sweep_log=""
pair_delta=""
wall_seconds=""
metrics_elapsed_ms=""
throttles=""
failed_requests=""
statistics_attempts=""
catalog_attempts=""
orders_attempts=""
drift_checked=false

journal="$("$JOURNALCTL" -u wfm-scrape.service -o json --no-pager -n 1000 2>/dev/null || true)"
if [ -z "$journal" ]; then
  error "no journal entries for wfm-scrape.service"
else
  mapfile -t jlines <<< "$journal"
  jts=(); jinv=(); jmsg=()
  for line in "${jlines[@]}"; do
    t=""; v=""; m=""
    if [[ "$line" =~ \"__REALTIME_TIMESTAMP\":\"([0-9]+)\" ]]; then t="${BASH_REMATCH[1]}"; fi
    if [[ "$line" =~ \"_SYSTEMD_INVOCATION_ID\":\"([0-9a-fA-F]+)\" ]]; then v="${BASH_REMATCH[1]}"; fi
    if [[ "$line" =~ \"MESSAGE\":\"([^\"]*)\" ]]; then m="${BASH_REMATCH[1]}"; fi
    jts+=("$t"); jinv+=("$v"); jmsg+=("$m")
  done

  metrics_index=-1
  for i in "${!jmsg[@]}"; do
    case "${jmsg[$i]}" in
      *"sweep metrics:"*)
        if printf '%s' "${jmsg[$i]}" | grep -q 'attempts\[catalog=[1-9]'; then
          metrics_index="$i"
        fi
        ;;
    esac
  done
  if [ "$metrics_index" -lt 0 ]; then
    for i in "${!jmsg[@]}"; do
      case "${jmsg[$i]}" in *"sweep metrics:"*) metrics_index="$i";; esac
    done
  fi

  if [ "$metrics_index" -lt 0 ]; then
    error "the journal holds no sweep metrics line"
  else
    metrics_found=true
    metrics_line="${jmsg[$metrics_index]}"
    invocation="${jinv[$metrics_index]}"
    if [ -z "$invocation" ]; then
      error "the most recent sweep metrics line carries no invocation id, so its duration cannot be established"
    else
      block_start="$metrics_index"
      while [ "$block_start" -gt 0 ] && [ "${jinv[$((block_start - 1))]}" = "$invocation" ]; do
        block_start=$((block_start - 1))
      done
      block_end="$metrics_index"
      while [ "$block_end" -lt $(( ${#jmsg[@]} - 1 )) ] && [ "${jinv[$((block_end + 1))]}" = "$invocation" ]; do
        block_end=$((block_end + 1))
      done

      ambiguous=false
      for i in "${!jinv[@]}"; do
        if [ "$i" -lt "$block_start" ] || [ "$i" -gt "$block_end" ]; then
          [ "${jinv[$i]}" = "$invocation" ] && ambiguous=true
        fi
      done
      if [ "$ambiguous" = true ]; then
        error "the journal interleaves invocation $invocation with another sweep - start and end cannot be paired"
      elif [ "$block_start" -eq 0 ]; then
        error "the journal window is truncated before the most recent sweep's start"
      elif [ "${jinv[$((block_start - 1))]}" != "" ]; then
        error "the most recent sweep's invocation is not contiguous in the journal"
      elif ! printf '%s' "${jmsg[$((block_start - 1))]}" | grep -q "Starting wfm-scrape.service"; then
        error "no start timestamp for the most recent sweep's invocation ($invocation)"
      else
        start_seconds=$(( ${jts[$((block_start - 1))]} / 1000000 ))
        end_seconds="$start_seconds"
        for i in $(seq "$block_start" "$block_end"); do
          seconds=$(( ${jts[$i]} / 1000000 ))
          [ "$seconds" -gt "$end_seconds" ] && end_seconds="$seconds"
        done
        if [ "$end_seconds" -ge "$start_seconds" ]; then
          wall_seconds=$((end_seconds - start_seconds))
        fi

        # The invocation is only evidence about the corpus if it ran the sweep
        # the corpus records: pair its start with the log written at that start.
        matched="$(awk -F'|' -v t="$start_seconds" -v tol="$PAIR_TOLERANCE_SECONDS" \
          'BEGIN { best=""; bd=1e18 } { d=$1-t; if (d<0) d=-d; if (d<bd) { bd=d; best=$0 } } END { if (best != "" && bd<=tol) print best }' \
          "$log_rows")"
        if [ -z "$matched" ]; then
          error "the most recent sweep's invocation does not pair with any observation log"
        else
          sweep_log="$(printf '%s' "$matched" | cut -d'|' -f2)"
          pair_delta=$(( start_seconds - $(printf '%s' "$matched" | cut -d'|' -f1) ))
          [ "$pair_delta" -lt 0 ] && pair_delta=$((-pair_delta))
        fi
      fi
    fi

    # Absent fields are reported as absent and failed, never as a skipped gate -
    # and never as an abort: these pipelines used to exit the whole run before a
    # report existed when the metrics line carried no such field.
    metrics_elapsed_ms="$(printf '%s' "$metrics_line" | grep -o 'elapsed_ms=[0-9]*' 2>/dev/null | head -1 | cut -d= -f2 || true)"
    throttles="$(printf '%s' "$metrics_line" | grep -o 'throttles=[0-9]*' 2>/dev/null | head -1 | cut -d= -f2 || true)"
    if [ -z "${metrics_elapsed_ms:-}" ]; then
      error "the most recent sweep metrics line reports no elapsed_ms"
    fi
    if [ -z "${throttles:-}" ]; then
      error "the most recent sweep metrics line reports no throttles count"
    fi
    attempts_body="$(printf '%s' "$metrics_line" | sed -n 's/.*attempts\[\([^]]*\)\].*/\1/p')"
    failed_body="$(printf '%s' "$metrics_line" | sed -n 's/.*failed\[\([^]]*\)\].*/\1/p')"
    statistics_attempts="$(printf '%s' "$attempts_body" | tr ' ' '\n' | sed -n 's/^statistics=\([0-9]*\)$/\1/p')"
    catalog_attempts="$(printf '%s' "$attempts_body" | tr ' ' '\n' | sed -n 's/^catalog=\([0-9]*\)$/\1/p')"
    orders_attempts="$(printf '%s' "$attempts_body" | tr ' ' '\n' | sed -n 's/^orders=\([0-9]*\)$/\1/p')"
    failed_requests="$(printf '%s' "$failed_body" | tr ' ' '\n' | sed -n 's/^[^=]*=\([0-9]*\)$/\1/p' | awk '{s+=$1} END {print s+0}')"

    if [ -n "${wall_seconds:-}" ]; then
      if [ "$wall_seconds" -ge "$URGENT_ELAPSED_SECONDS" ]; then
        error "the most recent sweep invocation took $((wall_seconds / 60)) minutes - close to the unit's timeout"
      elif [ "$wall_seconds" -ge "$WARN_ELAPSED_SECONDS" ]; then
        warn "the most recent sweep invocation took $((wall_seconds / 60)) minutes"
      fi
    fi
    attempts_total="$(printf '%s' "$attempts_body" | tr ' ' '\n' | sed -n 's/^[^=]*=\([0-9]*\)$/\1/p' | awk '{s+=$1} END {print s+0}')"
    failed_rate=0
    if [ "${attempts_total:-0}" -gt 0 ]; then
      failed_rate=$((failed_requests * 10000 / attempts_total))
    fi
    if [ "$failed_requests" -gt 0 ]; then
      if [ "$failed_rate" -gt 100 ]; then
        error "$failed_requests of $attempts_total requests failed (over 1%)"
      else
        warn "$failed_requests of $attempts_total requests failed"
      fi
    fi
    if [ "${throttles:-0}" -gt 0 ]; then
      if [ $((throttles * 10000)) -gt $((attempts_total * 100)) ]; then
        error "$throttles throttle(s) in the most recent sweep (over 1%)"
      else
        warn "$throttles throttle(s) in the most recent sweep"
      fi
    fi

    # The baseline is the sweep the invocation actually ran: one catalog fetch,
    # one statistics call per catalog item, one orders call per kept item. A
    # count more than 10% off that means requests were dropped or repeated, and
    # the observation rows that follow cannot be read as a cross-section. A zero
    # or absent count is not agreement - it is a missing measurement, and the
    # check cannot pass what it never saw.
    if [ -n "$sweep_log" ]; then
      baseline_items="$(printf '%s' "$matched" | cut -d'|' -f3)"
      baseline_kept="$(printf '%s' "$matched" | cut -d'|' -f4)"
      drift_checked=true
      if [ -n "${catalog_attempts:-}" ] && [ "$catalog_attempts" != 1 ]; then
        warn "the most recent sweep made $catalog_attempts catalog requests (baseline 1)"
      fi
      if [ -z "${baseline_items:-}" ]; then
        error "cannot check request drift: $sweep_log has no catalog size"
      elif [ -z "${statistics_attempts:-}" ] || [ "$statistics_attempts" -eq 0 ]; then
        error "the most recent sweep recorded no statistics requests against a $baseline_items-item catalog"
      else
        delta=$((statistics_attempts - baseline_items))
        [ "$delta" -lt 0 ] && delta=$((-delta))
        if [ $((delta * 10)) -gt "$baseline_items" ]; then
          error "the most recent sweep made $statistics_attempts statistics requests against a $baseline_items-item catalog"
        fi
      fi
      if [ -z "${baseline_kept:-}" ]; then
        error "cannot check orders requests: $sweep_log has no summary"
      elif [ -z "${orders_attempts:-}" ]; then
        error "the most recent sweep recorded no orders requests against $baseline_kept kept rows"
      else
        delta=$((orders_attempts - baseline_kept))
        [ "$delta" -lt 0 ] && delta=$((-delta))
        if [ $((delta * 10)) -gt "$baseline_kept" ]; then
          error "the most recent sweep made $orders_attempts orders requests against $baseline_kept kept rows"
        fi
      fi
    fi
  fi

  service_state="$("$SYSTEMCTL" show wfm-scrape.service -p ActiveState --value 2>/dev/null || true)"
  service_result="$("$SYSTEMCTL" show wfm-scrape.service -p Result --value 2>/dev/null || true)"
  case "${service_result:-}" in
    timeout) error "wfm-scrape.service timed out";;
    failed) error "wfm-scrape.service failed";;
  esac
  case "${service_state:-}" in
    failed) error "wfm-scrape.service is in a failed state";;
  esac
fi

# ---- timers -----------------------------------------------------------------

timer_enabled="$("$SYSTEMCTL" show wfm-scrape.timer -p UnitFileState --value 2>/dev/null || true)"
timer_active="$("$SYSTEMCTL" show wfm-scrape.timer -p ActiveState --value 2>/dev/null || true)"
next_elapse_raw="$("$SYSTEMCTL" show wfm-scrape.timer -p NextElapseUSecRealtime --value 2>/dev/null || true)"
[ "$timer_enabled" = enabled ] || error "wfm-scrape.timer is ${timer_enabled:-unknown}, not enabled"
[ "$timer_active" = active ] || error "wfm-scrape.timer is ${timer_active:-unknown}, not active"
next_elapse_epoch=""
case "${next_elapse_raw:-}" in
  ''|n/a) error "wfm-scrape.timer has no next elapse";;
  *) next_elapse_epoch="$(iso_to_epoch "$next_elapse_raw")" || true;;
esac
if [ -z "${next_elapse_epoch:-}" ]; then
  error "cannot read wfm-scrape.timer's next elapse"
elif [ $((next_elapse_epoch + BOUNDARY_GRACE_SECONDS)) -lt "$NOW_EPOCH" ]; then
  error "wfm-scrape.timer missed its ${next_elapse_raw} boundary by over 15 minutes"
fi

retired_timer="$("$SYSTEMCTL" is-enabled wfm-scrape-pull.timer 2>/dev/null || true)"
if [ "$retired_timer" = enabled ]; then
  error "the retired wfm-scrape-pull.timer is enabled"
fi

# ---- deployment provenance --------------------------------------------------

revision=""
recorded_sha=""
installed_sha=""
if [ -f "$DEPLOYED" ]; then
  # A read failure here still has to produce a report: the contract is that an
  # unreadable input is an error in the report, not a run that says nothing.
  record="$(cat "$DEPLOYED" || true)"
  revision="$(record_field "$record" revision)"
  recorded_sha="$(record_field "$record" sha256)"
  if [ -z "$revision" ] || [ -z "$recorded_sha" ]; then
    error "$DEPLOYED is missing its revision or sha256"
  fi
else
  error "$DEPLOYED is missing"
fi
if [ -f "$BINARY" ]; then
  installed_sha="$(sha256sum "$BINARY" 2>/dev/null | cut -d' ' -f1 || true)"
  if [ -n "$recorded_sha" ] && [ "$installed_sha" != "$recorded_sha" ]; then
    error "the installed scraper is not the recorded revision's binary"
  fi
else
  error "$BINARY is missing"
fi

# ---- preservation statement -------------------------------------------------
# The honest headline, and the only claim this check is entitled to make about
# preservation. In external-backup mode the protection is a host-level backup of
# the whole container, verified by an operator-run pull-and-verify job on another
# machine; nothing here can see it, so the report says declared-external and
# records that it is not independently verified from this host - it must never
# read as "preservation verified". In on-box-archive mode the receipt is the
# evidence, and the archive block above reports what it supports.

preservation_status="undeclared"
preservation_evidence="none"
preservation_note="preservation mode not declared - this check cannot say how the corpus is protected"
case "$PRESERVATION" in
  external-backup)
    preservation_status="declared-external"
    preservation_note="the corpus is protected by host-level Proxmox backups of this container, verified by an operator-run pull-and-verify job on another machine; this host cannot see them, and this check does not independently verify that protection"
    ;;
  on-box-archive)
    preservation_status="on-box-archive"
    preservation_evidence="archive-receipt"
    preservation_note="preservation rests on the archive host's receipt read through this box; the archive block reports what that receipt supports"
    ;;
esac

# ---- report -----------------------------------------------------------------

ready=true
[ "${#ERRORS[@]}" = 0 ] || ready=false

report="$(cat <<EOF
{
  "ready": $ready,
  "checked_at": "$NOW_ISO",
  "observations_dir": $(jstr "$OBSERVATIONS"),
  "errors": $(jarr "${ERRORS[@]+"${ERRORS[@]}"}"),
  "warnings": $(jarr "${WARNINGS[@]+"${WARNINGS[@]}"}"),
  "window": {
    "start": $(jstr_or_null "$oldest_start"),
    "end": $(jstr_or_null "$newest_start"),
    "span_seconds": $window_span,
    "complete_four_weeks": $complete_four_weeks
  },
  "sweeps": {
    "expected": $expected,
    "valid": $valid,
    "missing": $missing,
    "newest_age_seconds": $(jnum "$newest_age"),
    "snapshot_age_seconds": $(jnum "$snapshot_age"),
    "snapshot": $(jstr "$snapshot_state"),
    "longest_gap_seconds": $longest_gap,
    "per_day": {$per_day}
  },
  "anomalies": {
    "partial": $partials,
    "malformed": $malformed,
    "missing_summary": $missing_summary,
    "unsupported_format": $unsupported
  },
  "completeness": {
    "failed": $(jarr "${completeness_failures[@]+"${completeness_failures[@]}"}")
  },
  "csv": {
    "path": $(jstr "$CSV"),
    "status": $(jstr "$csv_state"),
    "rows": $(jnum "$csv_rows"),
    "sweep": $(jstr_or_null "$csv_sweep")
  },
  "retention": {
    "bytes": $total_bytes,
    "cap_bytes": $RETENTION_BYTES,
    "oldest_age_seconds": $oldest_age_display,
    "max_age_seconds": $RETENTION_AGE_SECONDS,
    "oldest_partial_age_seconds": $(jnum "$oldest_partial_age")
  },
  "disk": {
    "path": $(jstr "$disk_path"),
    "free_bytes": $(jnum "$free_bytes"),
    "floor_bytes": $DISK_FLOOR_BYTES
  },
  "archive": {
    "status": $(jstr "$archive_status"),
    "receipt": $(jstr "$ARCHIVE_RECEIPT"),
    "verified_at": $(jstr_or_null "$archive_verified"),
    "age_seconds": $(jnum "$archive_age"),
    "deadline_seconds": $ARCHIVE_DEADLINE_SECONDS,
    "covered": $(jnum "$archive_covered"),
    "overdue": $(jarr "${overdue[@]+"${overdue[@]}"}"),
    "hash_mismatch": $(jarr "${mismatched[@]+"${mismatched[@]}"}"),
    "deployed_revision_at_archive": $(jstr_or_null "$archive_receipt_revision"),
    "producer_revision_unknown": $archive_producer_unknown
  },
  "preservation": {
    "mode": $(jstr_or_null "$PRESERVATION"),
    "status": $(jstr "$preservation_status"),
    "independently_verified_from_this_host": false,
    "evidence_on_this_host": $(jstr "$preservation_evidence"),
    "note": $(jstr "$preservation_note")
  },
  "service": {
    "metrics": $metrics_found,
    "invocation": $(jstr_or_null "$invocation"),
    "state": $(jstr "$service_state"),
    "result": $(jstr "$service_result"),
    "wall_seconds": $(jnum "$wall_seconds"),
    "metrics_elapsed_ms": $(jnum "$metrics_elapsed_ms"),
    "sweep_log": $(jstr_or_null "$sweep_log"),
    "pair_delta_seconds": $(jnum "$pair_delta"),
    "drift_checked": $drift_checked,
    "throttles": $(jnum "$throttles"),
    "failed_requests": $(jnum "$failed_requests"),
    "statistics_attempts": $(jnum "$statistics_attempts"),
    "catalog_attempts": $(jnum "$catalog_attempts"),
    "orders_attempts": $(jnum "$orders_attempts")
  },
  "timers": {
    "sweep_enabled": $(jstr "$timer_enabled"),
    "sweep_active": $(jstr "$timer_active"),
    "next_elapse": $(jstr "$next_elapse_raw"),
    "retired_pull": $(jstr "$retired_timer")
  },
  "deployment": {
    "revision": $(jstr "$revision"),
    "recorded_sha256": $(jstr "$recorded_sha"),
    "installed_sha256": $(jstr "$installed_sha")
  }
}
EOF
)"

printf '%s\n' "$report"
if [ -n "$OUT" ]; then
  mkdir -p "$(dirname "$OUT")"
  printf '%s\n' "$report" > "$OUT.tmp"
  mv "$OUT.tmp" "$OUT"
fi

if [ "$ready" = true ]; then
  printf 'observations-check: ready (%s logs, %s bytes)\n' "$valid" "$total_bytes" >&2
  exit 0
fi
printf 'observations-check: NOT READY\n' >&2
for e in "${ERRORS[@]}"; do printf '  error: %s\n' "$e" >&2; done
exit 1
