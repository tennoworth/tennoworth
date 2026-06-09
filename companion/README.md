# wfm-fetch-inventory

Cross-platform companion (Linux + Windows) for the WF inventory market-check
web app. While Warframe is running, scrapes the game's process memory for the
session credentials it already obtained at login, then calls DE's
`inventory.php` endpoint and writes the response to your Downloads folder.

~3 MB binary, single file, no runtime deps. Rust.

## Run

While Warframe is running and you're past the login screen (opening the trade
or profile screen at least once guarantees the credentials are in memory):

**Linux** — needs ptrace access to `/proc/<pid>/mem`. Easiest:

```bash
sudo wfm-fetch-inventory
```

Or once, then no sudo per-run:

```bash
sudo setcap cap_sys_ptrace=eip ./wfm-fetch-inventory
./wfm-fetch-inventory
```

**Windows** — no elevation required if running as the same user that started
the game:

```powershell
.\wfm-fetch-inventory.exe
```

In both cases the inventory file ends up at `~/Downloads/inventory.json`.
When run via sudo on Linux, it's chown'd back to your user.

## Build

Local Linux build:

```bash
cd companion
cargo build --release
# Binary at target/release/wfm-fetch-inventory
```

Cross-compile to Windows from Linux (needs `mingw-w64-gcc`):

```bash
rustup target add x86_64-pc-windows-gnu
cargo build --release --target x86_64-pc-windows-gnu
```

Release artifacts are built in CI on tag push (`v*`) — see
`.github/workflows/release-companion.yml`. Linux releases are built on
`ubuntu-22.04` (glibc 2.35) for broad distro compatibility. Windows releases
build on a native `windows-latest` runner, no mingw needed.

## What it does, mechanically

1. `sysinfo` finds the running Warframe process by name.
2. On Linux, parses `/proc/<pid>/maps` and reads `/proc/<pid>/mem`.
   On Windows, walks the process address space with `VirtualQueryEx` +
   `ReadProcessMemory`.
3. Scans readable regions for three byte patterns:
   - `accountId=<24-hex>&nonce=<digits>` — your session credentials, as
     the game embeds them in URLs it sends.
   - `"BuildLabel":"<version>/<hash>"` — the build the inventory was issued
     for, so we can pass `appVersion` correctly.
   - `&ct=<2-4 letters>` — platform tag (`STM` Steam, `ME` Epic, `NS`
     Switch, etc.).
4. Picks the most-frequently-seen credential pair (defense against stale
   fragments still sitting in deallocated heap).
5. Calls `https://api.warframe.com/api/inventory.php` with those parameters
   and the build's User-Agent.
6. Writes the response to `<Downloads>/inventory.json`.

## What it doesn't do

- It doesn't print your `accountId` or `nonce` to stdout. Both are session
  secrets while you're logged in.
- It doesn't modify game memory or game state. It's a read-only memory
  inspector + an HTTP GET.
- It doesn't keep running in the background. One-shot — run it again when
  your inventory changes.

## Ban risk

EAC (the anti-cheat) does not, as of mid-2026, flag read-only memory
inspection of the game process. Sainan's
[warframe-api-helper](https://github.com/Sainan/warframe-api-helper) has used
the same approach for years with no documented bans, and AlecaFrame does the
equivalent via Overwolf. DE has not formally blessed this category of tool,
however. **Use at your own risk; there is no warranty.**

## License

MIT.
