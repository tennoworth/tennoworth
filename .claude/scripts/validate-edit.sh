#!/usr/bin/env bash
# Claude Code PostToolUse hook — runs after every Edit/Write/MultiEdit.
# Reads the hook JSON from stdin, picks the appropriate fast validator
# based on the edited file path, runs it, and exits non-zero on failure
# so the user gets a loud signal before the broken edit accumulates.
#
# Validators are chosen for speed (sub-second to ~3 s in the warm path):
#   - companion/**/*.rs         → cargo check (companion/)
#   - prototype/src/**/*.svelte → svelte-check (prototype/)
#   - prototype/src/**/*.{js,ts} → svelte-check (catches Svelte 5 reactivity + TS issues)
#
# Anything else is a no-op — we don't want to penalize edits to docs,
# config, or scratch files.
#
# Hook contract: exit 0 = pass (allow Claude to continue silently),
# exit 2 = soft block (Claude sees the failure and reacts).

set -u

ROOT="/home/prowly/Desktop/Warframe market check"

# Read the hook JSON from stdin. jq returns "null" (not empty) for missing
# keys; normalise to empty string.
input="$(cat)"
file_path="$(printf '%s' "$input" | jq -r '.tool_input.file_path // ""')"

# Path can also live under .tool_input.edits[].file_path for MultiEdit, but
# the single-file shape covers Edit/Write — which is what we care about for
# the validator triggers.

if [ -z "$file_path" ]; then
  exit 0
fi

# Strip the project root so we can pattern-match cleanly.
case "$file_path" in
  "$ROOT/"*) rel="${file_path#"$ROOT/"}" ;;
  *)         exit 0 ;;
esac

run_quiet() {
  # Run a command; on failure dump output to stderr and exit-2 (soft block).
  local out
  if ! out="$("$@" 2>&1)"; then
    printf '%s\n' "$out" >&2
    exit 2
  fi
}

case "$rel" in
  companion/src/*.rs)
    cd "$ROOT/companion" || exit 0
    run_quiet cargo check --quiet --tests
    ;;
  prototype/src/*.svelte|prototype/src/*.ts|prototype/src/*.js|prototype/src/**/*.svelte|prototype/src/**/*.ts|prototype/src/**/*.js)
    # svelte-check is fast after first run (~1–2 s cached). Local binary
    # avoids the `bunx`/`npx` version-check overhead.
    cd "$ROOT/prototype" || exit 0
    if [ -x node_modules/.bin/svelte-check ]; then
      run_quiet node_modules/.bin/svelte-check --threshold error
    fi
    ;;
  prototype/src/*.test.ts|prototype/src/*.test.js|prototype/src/**/*.test.ts|prototype/src/**/*.test.js)
    # When a test file specifically changes, run vitest once via bun
    # (faster cold-start than npm). Fall back to npm if bun isn't on PATH.
    cd "$ROOT/prototype" || exit 0
    if command -v bun >/dev/null 2>&1; then
      run_quiet bun run test
    else
      run_quiet npm test --silent
    fi
    ;;
  *)
    exit 0
    ;;
esac

exit 0
