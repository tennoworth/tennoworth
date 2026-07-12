# wfm-fetch-inventory

Cross-platform companion (Linux + Windows) for the WF inventory market-check
web app. While Warframe is running, scrapes the game's process memory for the
session credentials it already obtained at login, then calls DE's
`inventory.php` endpoint and writes the response to your Downloads folder.

~3 MB binary, single file, no runtime deps. Rust.

## Quick start

The binary has three subcommands. The order is:

1. **`fetch`** (default) — grab your inventory → `~/Downloads/inventory.json`,
   then drop it into the web app. This is all you need to see *what to sell*.
2. **`login`** (once) — sign in to warframe.market; encrypts your token at rest.
3. **`serve`** — run a local server so the web app can *create/edit listings*
   for you. Needed only for the list-on-WFM half.

### 1. Get your inventory (`fetch`)

While Warframe is running and you're past the login screen (opening the trade
or profile screen at least once guarantees the credentials are in memory):

**Linux** — needs ptrace access to `/proc/<pid>/mem`. Grant it once, then no
sudo per run (re-run after every rebuild — Linux wipes the capability when the
file is replaced):

```bash
sudo setcap cap_sys_ptrace=eip ./wfm-fetch-inventory
./wfm-fetch-inventory
```

Fallback if you can't `setcap` (e.g. ptrace_scope locked down): `sudo
./wfm-fetch-inventory` — the output file is chown'd back to your user.

**Windows** — no elevation and no permission grant required; just run it from a
normal PowerShell as the same user that started the game:

```powershell
.\wfm-fetch-inventory.exe
```

The inventory file ends up at `~/Downloads/inventory.json`
(`C:\Users\<you>\Downloads\inventory.json` on Windows).

### 2 & 3. List on warframe.market (`login` → `serve`)

```bash
wfm-fetch-inventory login    # once — interactive sign-in, sets a passphrase
wfm-fetch-inventory serve    # leave running; prints a URL to paste into the app
```

`serve` **must run in a real terminal window** — it prompts for the passphrase
you set at `login`. From a non-terminal context (IDE run button, `nohup`,
systemd) it fails with `reading passphrase / No such device or address (os
error 6)`; pipe the passphrase instead:

```bash
printf %s 'your-passphrase' | wfm-fetch-inventory serve --passphrase-stdin
```

On start, `serve` **opens your browser pre-connected** to the app — no
copy-paste — and the app pulls your inventory straight from the server (no
file). The token rides in the URL fragment, which never leaves your machine.

If the browser doesn't open (headless box, `--no-open`), `serve` also prints a
line like `http://127.0.0.1:49xxx?token=…`; paste that **whole line** into the
web app's Companion tab. The port is **random and changes every run** — it is
*not* the website's `5173`, and the token rotates each run too.

`serve` also exposes `GET /inventory`, so the app's "Pull / Refresh inventory"
button memory-scans the running game on demand — you never touch a file. (That
route uses only the in-memory session creds, never your JWT.)

### Flags

| Flag | Subcommand | Purpose |
|---|---|---|
| `--port <N>` | serve | Pin the port (default `0` = random free port). |
| `--passphrase-stdin` | serve | Read the passphrase from stdin (no TTY). |
| `--app-url <url>` | serve | App URL to open pre-connected (default the site; use `http://127.0.0.1:5173` for dev). |
| `--no-open` | serve | Don't open a browser on start (headless / remote). |
| `--pid <N>` | fetch | Target a specific Warframe PID (multi-match). |
| `--out <path>` | fetch | Override the inventory.json output path. |
| `--platform <p>` | login | Account platform (`pc`/`switch`/…). |
| `--jwt-path <path>` | login/serve | Override the encrypted-JWT location. |

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
- `fetch` doesn't keep running — it's one-shot, run it again when your
  inventory changes. (`serve`, by contrast, runs until you Ctrl-C it.)

## Ban risk

EAC (the anti-cheat) does not, as of mid-2026, flag read-only memory
inspection of the game process. Sainan's
[warframe-api-helper](https://github.com/Sainan/warframe-api-helper) has used
the same approach for years with no documented bans, and AlecaFrame does the
equivalent via Overwolf. DE has not formally blessed this category of tool,
however. **Use at your own risk; there is no warranty.**

## License

MIT.
