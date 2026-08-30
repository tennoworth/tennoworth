#!/usr/bin/env bash
# OnFailure handler. Invoked by systemd as `wfm-alert@<failed-unit>.service`,
# so $1 is the unit that failed.
#
# Exists because two separate outages ran for days without anyone noticing:
# the dnf repo publisher failed at `rpm --addsign` for 9 days while apt kept
# working, and wfm-app-pull failed on its very first timer firing. Both sat in
# `systemctl --failed`, which nothing looks at. A cron box with no alerting is
# a box whose failures are discovered by accident.
#
# Two sinks, deliberately:
#   - an append-only log OUTSIDE the journal, because the journal rotates and
#     these are exactly the events you go looking for weeks later;
#   - an optional webhook, so the box can actually reach out. Unset by default
#     - no channel is assumed and nothing fails if none is configured.
#
# Must never fail: an OnFailure handler that itself fails just adds a second
# failed unit and buries the first.
set -uo pipefail

UNIT="${1:-unknown.service}"
LOG="${WFM_ALERT_LOG:-/srv/wfm/alerts.log}"
ENV_FILE="${WFM_ALERT_ENV:-/etc/wfm-alert.env}"

# Optional config: ALERT_WEBHOOK_URL=... (anything that accepts a POST body -
# ntfy, a Discord/Slack webhook, healthchecks.io's /fail endpoint).
# shellcheck disable=SC1090
[ -r "$ENV_FILE" ] && . "$ENV_FILE"

stamp=$(date -u +%Y-%m-%dT%H:%M:%SZ)
result=$(systemctl show "$UNIT" -p Result --value 2>/dev/null || echo unknown)
# The last few lines are almost always the actual error; the full journal is
# still there for anyone who wants it.
context=$(journalctl -u "$UNIT" --no-pager -o cat -n 12 2>/dev/null | tail -12)

{
  echo "=== $stamp  $UNIT  result=$result"
  printf '%s\n' "$context" | sed 's/^/    /'
} >> "$LOG" 2>/dev/null || true

# Journal too, with a fixed prefix so `journalctl -t wfm-alert` and a grep for
# WFM_ALERT both find every incident.
echo "WFM_ALERT $UNIT failed (result=$result) at $stamp" >&2

if [ -n "${ALERT_WEBHOOK_URL:-}" ]; then
  body=$(printf 'TennoWorth box: %s failed (result=%s) at %s\n\n%s' \
    "$UNIT" "$result" "$stamp" "$context")
  # --max-time so a hung endpoint cannot wedge the handler; failure to notify
  # is logged and swallowed, never propagated.
  curl -fsS --max-time 15 -X POST -H 'Content-Type: text/plain' \
    --data-binary "$body" "$ALERT_WEBHOOK_URL" >/dev/null 2>&1 \
    || echo "WFM_ALERT webhook POST failed for $UNIT" >&2
fi

exit 0
