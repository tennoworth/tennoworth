#!/usr/bin/env bash
# Hourly health check for the market host: is the sweep scheduled, and is the
# installed pipeline the one main says it should be?
#
# Two failures go unnoticed without it. A sweep timer that is disabled, stopped
# or past its boundary never fails a unit, so OnFailure never fires. And the
# pipeline is installed only by scripts/deploy-scrape-host.sh, by hand, while
# the checkout follows main on its own: on 2026-10-01 the box ran 03ff5b0 three
# pipeline merges behind main and nothing said so. A failed or timed-out sweep
# already alerts through tennoworth-scrape.service's own OnFailure.
#
# Fails closed: missing evidence is an error, not an empty result.
#
# Runs on the box from tennoworth-host-check.timer, and from the private
# dashboard's "Check the host" job.
#
# Exit status: 0 healthy, 1 not healthy (a report is still written), 2 the check
# could not run at all.
set -euo pipefail

usage() {
  cat <<'USAGE'
usage: host-check.sh [options]

  --out <json>                write the report here (always also printed to stdout)
  --deployed <path>           deployment record (default /srv/wfm/deployed.json)
  --binary <path>             installed pipeline (default /srv/wfm/bin/tennoworth-pipeline)
  --app <dir>                 the box's checkout of main, which the deployed
                              pipeline is compared against (default /srv/wfm/app)
  --deploy-grace-hours <n>    how long pipeline changes in the checkout may wait
                              for a deploy before the host is not healthy (default 24)
  --csv <path>                the sweep's CSV (default /srv/wfm/data/scrape/wfm_results.csv)
  --snapshot <path>           published snapshot (default /srv/wfm/data/public/market.json)
  --disk-path <dir>           filesystem whose free space is checked (default /srv/wfm)
  --disk-floor-bytes <n>      free-space floor (default 1073741824)
  --now <iso>                 evaluation instant, for reproducing a report
USAGE
}

OUT=""
DEPLOYED="/srv/wfm/deployed.json"
BINARY="/srv/wfm/bin/tennoworth-pipeline"
APP="/srv/wfm/app"
DEPLOY_GRACE_HOURS=24
CSV="/srv/wfm/data/scrape/wfm_results.csv"
SNAPSHOT="/srv/wfm/data/public/market.json"
DISK_PATH="/srv/wfm"
DISK_FLOOR_BYTES=1073741824
BOUNDARY_GRACE_SECONDS=900
NOW=""
SYSTEMCTL="${SYSTEMCTL:-systemctl}"

while [ $# -gt 0 ]; do
  case "$1" in
    --out) OUT="$2"; shift 2;;
    --deployed) DEPLOYED="$2"; shift 2;;
    --binary) BINARY="$2"; shift 2;;
    --app) APP="$2"; shift 2;;
    --deploy-grace-hours) DEPLOY_GRACE_HOURS="$2"; shift 2;;
    --csv) CSV="$2"; shift 2;;
    --snapshot) SNAPSHOT="$2"; shift 2;;
    --disk-path) DISK_PATH="$2"; shift 2;;
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

# /srv/wfm/deployed.json is pretty-printed, hence the whitespace tolerance.
# Absence is a result, not a failure: with `set -o pipefail` a grep that finds
# no match would abort the run before any report existed.
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
  NOW_EPOCH="$(date -u +%s)"
fi
NOW_ISO="$(epoch_to_iso "$NOW_EPOCH")"

# ---- the sweep unit ---------------------------------------------------------

service_state="$("$SYSTEMCTL" show tennoworth-scrape.service -p ActiveState --value 2>/dev/null || true)"
service_result="$("$SYSTEMCTL" show tennoworth-scrape.service -p Result --value 2>/dev/null || true)"
case "${service_result:-}" in
  timeout) error "tennoworth-scrape.service timed out";;
  failed) error "tennoworth-scrape.service failed";;
esac
case "${service_state:-}" in
  failed) error "tennoworth-scrape.service is in a failed state";;
esac

# ---- sweep output and disk --------------------------------------------------

csv_rows=""
if [ -f "$CSV" ]; then
  csv_rows="$(tail -n +2 "$CSV" 2>/dev/null | grep -c . || true)"
  [ "${csv_rows:-0}" -gt 0 ] || error "$CSV holds no rows"
else
  error "$CSV is missing"
fi
snapshot_age=""
if [ -f "$SNAPSHOT" ]; then
  snapshot_age=$((NOW_EPOCH - $(stat -c %Y "$SNAPSHOT" 2>/dev/null || printf '%s' "$NOW_EPOCH")))
else
  error "$SNAPSHOT is missing"
fi
free_bytes="$(df -P -B1 "$DISK_PATH" 2>/dev/null | awk 'NR==2 {print $4}' || true)"
if [ -z "${free_bytes:-}" ]; then
  error "cannot read free space on $DISK_PATH"
elif [ "$free_bytes" -lt "$DISK_FLOOR_BYTES" ]; then
  error "only $free_bytes bytes free on $DISK_PATH, under the $DISK_FLOOR_BYTES floor"
fi

# ---- timers -----------------------------------------------------------------

timer_enabled="$("$SYSTEMCTL" show tennoworth-scrape.timer -p UnitFileState --value 2>/dev/null || true)"
timer_active="$("$SYSTEMCTL" show tennoworth-scrape.timer -p ActiveState --value 2>/dev/null || true)"
next_elapse_raw="$("$SYSTEMCTL" show tennoworth-scrape.timer -p NextElapseUSecRealtime --value 2>/dev/null || true)"
[ "$timer_enabled" = enabled ] || error "tennoworth-scrape.timer is ${timer_enabled:-unknown}, not enabled"
[ "$timer_active" = active ] || error "tennoworth-scrape.timer is ${timer_active:-unknown}, not active"
# While its sweep runs, systemd reports no next elapse for the timer: the next
# one is scheduled when the run ends. Reading that as a broken timer turned
# every check that landed mid-sweep - every other hour - into a false alert.
sweep_running=false
case "${service_state:-}" in
  activating|active|deactivating|reloading) sweep_running=true;;
esac
next_elapse_epoch=""
case "${next_elapse_raw:-}" in
  ''|n/a) [ "$sweep_running" = true ] || error "tennoworth-scrape.timer has no next elapse";;
  *) next_elapse_epoch="$(iso_to_epoch "$next_elapse_raw")" || true;;
esac
if [ -z "${next_elapse_epoch:-}" ]; then
  [ "$sweep_running" = true ] || error "cannot read tennoworth-scrape.timer's next elapse"
elif [ $((next_elapse_epoch + BOUNDARY_GRACE_SECONDS)) -lt "$NOW_EPOCH" ]; then
  error "tennoworth-scrape.timer missed its ${next_elapse_raw} boundary by over 15 minutes"
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
    error "the installed pipeline is not the recorded revision's binary"
  fi
else
  error "$BINARY is missing"
fi

# A change still inside the grace window is a warning; past it, the host is not
# healthy until the deploy happens. A revision the checkout does not know was
# deployed from develop ahead of main, which is not lag.
# rust/Cargo.lock is judged separately below: every desktop release bumps the
# desktop package's own entry there, which says nothing about the pipeline.
SCRAPER_SOURCES=(
  rust/wfm-scrape rust/wfm-client rust/market-domain rust/Cargo.toml
  deploy/run-scrape.sh deploy/tennoworth-scrape.service deploy/tennoworth-scrape.timer
  deploy/host-check.sh deploy/tennoworth-host-check.service deploy/tennoworth-host-check.timer
)
# Root reads a checkout owned by wfm, which git refuses without this.
app_git() { git -c safe.directory="$APP" -C "$APP" "$@"; }
deploy_lag="unknown"
pending_commits=0
checkout_head=""
if [ -z "$revision" ]; then
  :
elif ! command -v git >/dev/null 2>&1 || [ ! -e "$APP/.git" ]; then
  warn "cannot compare the deployed scraper with $APP: no git checkout there"
elif ! checkout_head="$(app_git rev-parse --short HEAD 2>/dev/null)"; then
  checkout_head=""
  warn "cannot read the checkout at $APP"
elif ! app_git cat-file -e "$revision^{commit}" 2>/dev/null; then
  deploy_lag="ahead"
elif ! app_git merge-base --is-ancestor "$revision" HEAD 2>/dev/null; then
  deploy_lag="diverged"
  warn "the deployed scraper ${revision:0:7} is not an ancestor of the checkout's $checkout_head"
else
  pending="$(app_git log --format='%ct' "$revision..HEAD" -- "${SCRAPER_SOURCES[@]}" 2>/dev/null || true)"
  # Cargo.lock counts only where the scraper's own dependency closure changed.
  # A desktop version bump, or a dependency only the desktop pulls in, leaves it
  # alone, and so does every other lock commit in the range: judging per commit
  # keeps an old desktop bump from starting the grace clock for a newer change.
  scraper_closure() {
    # Keyed by name and version: the lock writes a dependency as "name" when it
    # holds one version of that crate and as "name version" when it holds
    # several, so following a bare name could pull a version only the desktop
    # uses into the scraper's closure.
    awk 'BEGIN { RS = ""; ORS = "\n\n" }
      {
        n = ""; v = ""
        if (match($0, /\nname = "[^"]+"/)) n = substr($0, RSTART + 9, RLENGTH - 10)
        if (match($0, /\nversion = "[^"]+"/)) v = substr($0, RSTART + 12, RLENGTH - 13)
        key = n " " v
        # A package identity, not its text: how the lock spells a dependency
        # changes when a second version of that crate appears elsewhere.
        ident = ""
        nl = split($0, lines, "\n")
        for (i = 1; i <= nl; i++) if (lines[i] ~ /^(name|version|source|checksum) = /) ident = ident lines[i] "\n"
        rec[NR] = ident; keyof[NR] = key
        versions[n]++; only[n] = key
        in_deps = 0
        for (i = 1; i <= nl; i++) {
          line = lines[i]
          if (line ~ /^dependencies = \[/) { in_deps = 1; continue }
          if (in_deps && line ~ /^\]/) { in_deps = 0; continue }
          if (in_deps) {
            gsub(/^ *"|",? *$/, "", line); k = split(line, parts, " ")
            deps[key] = deps[key] "\t" (k >= 2 ? parts[1] " " parts[2] : parts[1])
          }
        }
      }
      function resolve(d) { return (index(d, " ") ? d : (versions[d] == 1 ? only[d] : "")) }
      END {
        split("wfm-scrape wfm-client market-domain", roots, " ")
        for (r in roots) { k = resolve(roots[r]); if (k != "" && !(k in want)) { want[k] = 1; queue[++q] = k } }
        for (h = 1; h <= q; h++) {
          m = split(deps[queue[h]], ds, "\t")
          for (j = 1; j <= m; j++) {
            if (ds[j] == "") continue
            k = resolve(ds[j])
            if (k != "" && !(k in want)) { want[k] = 1; queue[++q] = k }
          }
        }
        for (i = 1; i <= NR; i++) if (keyof[i] in want) print rec[i]
      }'
  }
  lock_changes=""
  while read -r commit committed; do
    [ -n "$commit" ] || continue
    if ! cmp -s <(app_git show "$commit^:rust/Cargo.lock" 2>/dev/null | scraper_closure) \
                <(app_git show "$commit:rust/Cargo.lock" 2>/dev/null | scraper_closure); then
      lock_changes="$lock_changes$committed"$'\n'
    fi
  done < <(app_git log --format='%H %ct' "$revision..HEAD" -- rust/Cargo.lock 2>/dev/null || true)
  if [ -n "$lock_changes" ]; then
    pending="$(printf '%s\n%s' "$pending" "$lock_changes" | grep -v '^$' | sort -u || true)"
  fi
  if [ -z "$pending" ]; then
    deploy_lag="current"
  else
    deploy_lag="behind"
    pending_commits="$(printf '%s\n' "$pending" | wc -l)"
    oldest_pending="$(printf '%s\n' "$pending" | sort -n | head -1)"
    lag_message="the deployed scraper ${revision:0:7} is $pending_commits scraper commit(s) behind the checkout's $checkout_head"
    if [ $((NOW_EPOCH - oldest_pending)) -gt $((DEPLOY_GRACE_HOURS * 3600)) ]; then
      error "$lag_message; deploy it with scripts/deploy-scrape-host.sh"
    else
      warn "$lag_message"
    fi
  fi
fi

# ---- report -----------------------------------------------------------------

ready=true
[ "${#ERRORS[@]}" = 0 ] || ready=false

report="$(cat <<EOF
{
  "ready": $ready,
  "checked_at": "$NOW_ISO",
  "errors": $(jarr "${ERRORS[@]+"${ERRORS[@]}"}"),
  "warnings": $(jarr "${WARNINGS[@]+"${WARNINGS[@]}"}"),
  "service": {
    "state": $(jstr "$service_state"),
    "result": $(jstr "$service_result")
  },
  "sweep": {
    "csv_rows": $(jnum "$csv_rows"),
    "snapshot_age_seconds": $(jnum "$snapshot_age")
  },
  "disk": {
    "path": $(jstr "$DISK_PATH"),
    "free_bytes": $(jnum "$free_bytes"),
    "floor_bytes": $DISK_FLOOR_BYTES
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
    "installed_sha256": $(jstr "$installed_sha"),
    "checkout_head": $(jstr_or_null "$checkout_head"),
    "lag": $(jstr "$deploy_lag"),
    "pending_scraper_commits": $pending_commits
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
  printf 'host-check: healthy\n' >&2
  exit 0
fi
printf 'host-check: NOT HEALTHY\n' >&2
for e in "${ERRORS[@]}"; do printf '  error: %s\n' "$e" >&2; done
exit 1
