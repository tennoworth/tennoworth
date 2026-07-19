# wfm-fetch-inventory

Cross-platform companion (Linux + Windows) for the WF inventory market-check
web app. While Warframe is running, scrapes the game's process memory for the
session credentials it already obtained at login, then calls DE's
`inventory.php` endpoint and writes the response to the directory you ran
it from.

~3 MB binary, single file, no runtime deps. Rust.

## Quick start

Easiest path — **just run `serve`**:

```bash
wfm-fetch-inventory serve    # leave it running; your browser opens on the sell list
```

It opens the web app pre-connected and pulls your inventory straight from the
running game — no file, no login, no copy-paste. Leave it running; close the
terminal (or Ctrl-C) when you're done. Refreshing your inventory later is one
click in the app.

The three subcommands:

1. **`serve`** — the recommended default. Local server the web app talks to;
   opens your browser pre-connected and streams your inventory on demand.
   Works with **no login** — creating/editing warframe.market listings is the
   only part that needs one, and it unlocks the first time you use it.
2. **`fetch`** (default action) — the no-server alternative: grab your inventory
   to `./inventory.json` once and drop it into the web app by hand.
3. **`login`** (once) — sign in to warframe.market so `serve` can list items for
   you. Optional; only needed for the list-on-WFM half.

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

The inventory file lands in the directory you ran the command from
(override with `--out <path>`).

### Run the server (`serve`)

```bash
wfm-fetch-inventory serve    # leave running; opens your browser on the sell list
```

On start, `serve` **opens your browser pre-connected** to the app — no
copy-paste — and the app pulls your inventory straight from the server (no
file). The token rides in the URL fragment, which never leaves your machine.

If the browser doesn't open (headless box, `--no-open`), `serve` also prints a
line like `http://127.0.0.1:49xxx?token=…`; paste that **whole line** into the
web app's Companion tab. The port is **random and changes every run** — it is
*not* the website's `5173`, and the token rotates each run too.

The `GET /inventory` route it exposes uses only the in-memory game creds — never
your JWT — so the sell list and the app's "Pull / Refresh inventory" button work
with no login at all.

### List on warframe.market (`login`, then serve unlocks on first use)

Creating/editing listings needs a warframe.market login. Run it once:

```bash
wfm-fetch-inventory login    # interactive sign-in; sets a passphrase
```

After that, `serve` **starts without asking for anything** — it only prompts for
your passphrase (in the terminal where it's running) the first time you actually
list something. Until then, listing sits ready but locked.

From a non-terminal context (IDE run button, `nohup`, systemd) that first-use
prompt can't appear; pipe the passphrase at startup instead and listing unlocks
immediately:

```bash
printf %s 'your-passphrase' | wfm-fetch-inventory serve --passphrase-stdin
```

(Inventory pull still works without any of this — login only gates listing.)

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
6. Writes the response to `./inventory.json` (the invocation directory).

## What it doesn't do

- It doesn't print your `accountId` or `nonce` to stdout. Both are session
  secrets while you're logged in.
- It doesn't modify game memory or game state. It's a read-only memory
  inspector + an HTTP GET.
- `fetch` doesn't keep running — it's one-shot, run it again when your
  inventory changes. (`serve`, by contrast, runs until you Ctrl-C it.)

## Ban risk

The companion only ever *reads* memory — it never writes to the game, never
injects code, and doesn't interact with anti-cheat. **We cannot promise this
is ban-safe.** Sainan's
[warframe-api-helper](https://github.com/Sainan/warframe-api-helper) has used
the same read-only approach for years with no documented bans, and AlecaFrame
does the equivalent via Overwolf — but DE has never formally blessed this
category of tool. **Use at your own risk; there is no warranty.**

## License

MIT.
