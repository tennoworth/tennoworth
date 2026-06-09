---
description: Run all tests + builds across companion (Rust), prototype (Svelte), and Python. Report pass/fail per domain.
---

Run these in parallel — they're independent and have no shared state.
Use multiple Bash tool calls in a single message.

1. `cd "/home/prowly/Desktop/Warframe market check/companion" && cargo test --release --quiet`
2. `cd "/home/prowly/Desktop/Warframe market check/companion" && cargo build --release --quiet 2>&1 | tail -5`
3. `cd "/home/prowly/Desktop/Warframe market check/prototype" && bun run test 2>&1 | tail -8`
4. `cd "/home/prowly/Desktop/Warframe market check/prototype" && npx svelte-check 2>&1 | tail -5`
5. `cd "/home/prowly/Desktop/Warframe market check/prototype" && bun run build 2>&1 | tail -5`
6. `/home/prowly/.local/bin/pytest /home/prowly/Desktop/Warframe\ market\ check/tests/ -q 2>&1 | tail -5`

Report results as a short table:

| Check | Result |
|---|---|
| Rust tests | ✓ / ✗ + count |
| Rust build | ✓ / ✗ |
| Vitest | ✓ / ✗ + count |
| svelte-check | ✓ / ✗ |
| Vite build | ✓ / ✗ |
| Pytest | ✓ / ✗ + count |

If anything fails, paste the failure block under the table and STOP —
do not auto-fix unless the user asks.
