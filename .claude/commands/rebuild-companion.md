---
description: Rebuild the Rust companion in release mode. Reminds the user to restart any running serve process.
---

1. Run `cd "/home/prowly/Desktop/Warframe market check/companion" && cargo build --release 2>&1 | tail -5`.

2. Check for a running `wfm-fetch-inventory` process:
   `pgrep -af wfm-fetch-inventory`

3. If something is running, **do not kill it** without asking — that
   process holds a decrypted JWT in memory and the user may be
   mid-action. Instead, tell the user:

   > Build succeeded. Process `<PID>` is still running the previous
   > binary. To pick up the new code, restart it:
   > ```
   > kill <PID>
   > ! companion/target/release/wfm-fetch-inventory serve
   > ```
   > (The `!` prefix runs in your shell so the passphrase prompt
   > works.)

4. If nothing is running, just confirm the build succeeded.

5. If `cargo build` itself failed, paste the failure block and stop.
