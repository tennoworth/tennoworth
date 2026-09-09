#!/bin/sh
# Policy deployment is independent of site and desktop releases.
set -eu
DIRECTORY=${WFM_POLICY_DIRECTORY:-/srv/wfm/policy}
VERIFIER=${WFM_POLICY_VERIFIER:-/srv/wfm/bin/wfm-policy}
mkdir -p "$DIRECTORY"
exec 9>"$DIRECTORY/.pull.lock"
flock -n 9 || exit 0
TASK_DIR=$(mktemp -d "$DIRECTORY/.download-XXXXXX")
trap 'rm -rf "$TASK_DIR"' EXIT
curl --fail --silent --show-error --location --max-time 15 --max-filesize 65536 \
  https://github.com/tennoworth/tennoworth/releases/download/wfm-policy-latest/wfm-policy.json \
  -o "$TASK_DIR/wfm-policy.json"
if [ -f "$DIRECTORY/wfm-policy.json" ]; then
  cmp -s "$TASK_DIR/wfm-policy.json" "$DIRECTORY/wfm-policy.json" && exit 0
  "$VERIFIER" "$TASK_DIR/wfm-policy.json" "$DIRECTORY/wfm-policy.json"
else
  "$VERIFIER" "$TASK_DIR/wfm-policy.json"
fi
chmod 644 "$TASK_DIR/wfm-policy.json"
mv "$TASK_DIR/wfm-policy.json" "$DIRECTORY/wfm-policy.json"
