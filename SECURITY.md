# Security

This document is the honest threat model. It distinguishes what we
actually commit to from what we can't promise.

## Trust boundaries

The product has three components with three different trust
characteristics:

1. **The web app** (`prototype/`, deployed as static files).
   Pure client-side informational site. No backend. We see no inventory
   data, no credentials, no telemetry. Compromising the static host gives an
   attacker the ability to serve malicious JS to visitors.

2. **The desktop app** (`companion/tennoworth-desktop`, Rust + Tauri,
   distributed via GitHub releases and the Linux distro repos). Runs on the
   user's machine. Reads the game's process memory (Linux: needs
   `CAP_SYS_PTRACE`; Windows: same-user process access). Scans are
   performed in-process over Tauri IPC — there is no loopback HTTP server,
   no session token, and the browser webview never holds the WFM JWT, which
   stays in the Rust process and is encrypted at rest (AES-256-GCM,
   PBKDF2-600k passphrase). The core logic it drives lives in
   `companion/wfm-core`.

3. **Our build + release pipeline** (GitHub Actions). Three workflows:
   - `refresh-market.yml` — scrapes warframe.market once daily and
     commits a static `market.json` + `wfstat-catalog.json` to the
     repo (a floor so a fresh clone starts with recent data; the
     self-host box's own systemd timer covers the real 2 h cadence).
   - `build-web.yml` — on a push touching `prototype/`, builds the
     static web bundle and publishes it as a rolling `web-latest`
     prerelease asset (the self-host box pulls it with a plain curl).
   - `audit.yml` — on push / PR and weekly, runs dependency advisories
     (`bun audit`, `cargo audit`) plus the JS and Rust test
     suites.

   The `release-desktop.yml` workflow builds the desktop app (Windows
   installers, the Linux deb/rpm/AppImage and the raw binary tarball),
   publishes the versioned release and the updater manifest, and pushes
   the AUR packages. It is triggered manually from `main` with a version,
   never by a tag: the build jobs publish nothing, and a single job
   verifies the complete artifact set before flipping one draft release
   public. The `desktop-v*` tag is created **by** that workflow at the
   commit it built, so a release tag can only ever name a verified build
   of a `main` commit.

   Production serving is **not** GitHub-hosted: a self-hosted box (an
   unprivileged LXC, reached only through a Cloudflare Tunnel, fronted
   by Caddy) pulls the CI-built web bundle and runs its own scrape
   timer. That box is a trust boundary the repo's public CI does not
   cover — compromising it would let an attacker serve malicious JS or
   a stale snapshot to visitors.

## What we commit to

- **The web app does not exfiltrate your inventory.** All processing
  is in your browser, and there are **zero third-party origins** in
  the CSP. The only network calls are `GET /market.json` and
  `GET /wfstat-catalog.json` from our own origin (static files; the
  item-name catalog used to come from warframestat.us directly, but
  it's baked at build time since 2026-06).
- **The desktop app does not transmit your accountId or nonce.** They
  are scraped from game memory and used as URL parameters in a single
  HTTPS request to `api.warframe.com/api/inventory.php`, then
  discarded.
- **The WFM JWT never reaches the webview.** Login is handled in the
  Rust process; the encrypted token lives on disk and is decrypted
  only in memory. Listing and order operations are relayed by
  wfm-core — the webview only sees results.
- **Release binaries are built in public, auditable CI — never on a
  maintainer's machine.** You can read the workflow file, the source
  commit at the tag, and the full build logs for the run that produced
  every asset. What that is *not* is a **reproducible** build: the Rust
  toolchain floats on `stable` and nothing verifies that a rebuild
  produces byte-identical output, so you cannot independently recreate
  an installer and diff it. Auditable, not reproducible — this used to
  say "reproducibly built", which was a stronger promise than the
  pipeline keeps. Linux packages are signed (see below).
- **No telemetry, no analytics, no accounts.** Verify with your
  browser's network tab.

## The AI assistant (dormant)

The DeepSeek advisor relay exists in `wfm-core` and a dormant
`ask_assistant` Tauri command is registered, but **no UI surfaces it** —
there is no chat button, no key-setting path, and nothing sends data to
DeepSeek from the shipped app. It was last wired to the loopback companion's
`/assistant` route, which no longer exists. The code stays for a future
desktop assistant; until one ships, the feature is off by construction. If it
is ever re-enabled, the SECURITY.md section describing its data flow must be
rewritten before it ships.

## What we cannot promise

- **We cannot promise this is ban-safe.** The desktop app reads game
  process memory. Equivalent tools (Sainan's `warframe-api-helper`,
  AlecaFrame via Overwolf) have run for years without documented
  bans, but Digital Extremes has never formally blessed the category.
- **We cannot promise warframe.market won't change.** The scraping
  workflow and the listing endpoints depend on undocumented
  community-API behavior.
- **We cannot promise that a malicious clone of our site doesn't
  exist.** Always verify the URL. Don't enter your WFM credentials
  into anything that isn't the published desktop app.

## How to verify a release

For each desktop release on GitHub:

- **Windows** — the installer (`.exe` / `.msi`) is built in public CI
  from the tagged commit; download from the `desktop-v*` release and
  compare its SHA-256 with the `.sha256` file on the same release.
  (Windows `.sha256` sidecars start with the first release after 0.3.8.
  Up to and including 0.3.8 the installers shipped with only the
  updater's `.sig`, and this section described a file that did not
  exist — the Linux artifacts always had theirs.)
- **Linux** — prefer your distro's signed repository (apt/dnf); the
  `.deb` / `.rpm` on the release are what the repo publisher consumes.
  The AUR `tennoworth-bin` package pins the checksum of its tarball.

The `.sha256` files are plain `sha256sum` output — the hash, then the
filename it belongs to — so the check is one command wherever you have
`sha256sum` (Git Bash, WSL, or any Linux shell). Download the installer
and its `.sha256` into the same directory, then:

```bash
# Windows — the NSIS installer (substitute the version you downloaded):
sha256sum -c TennoWorth_0.4.0_x64-setup.exe.sha256

# Windows — the MSI, if you took that one instead:
sha256sum -c TennoWorth_0.4.0_x64_en-US.msi.sha256

# Linux — the .deb, if you are not using the apt repo:
sha256sum -c tennoworth-desktop-amd64.deb.sha256
```

In PowerShell with no `sha256sum` available, compare by eye instead:

```powershell
Get-FileHash .\TennoWorth_0.4.0_x64-setup.exe -Algorithm SHA256
Get-Content .\TennoWorth_0.4.0_x64-setup.exe.sha256
```

`sha256sum -c` prints `OK` when the file matches. Anything else — a
`FAILED` line, or two hashes that differ — means the file is corrupt or
tampered: delete it and re-download. Don't run a binary that fails this
check.

(The `.sig` files next to the installers are a different thing: minisign
signatures used by the in-app updater, verified against the public key
compiled into the app. They are not something you check by hand — the
`.sha256` is.)

## The Linux package signing key

The apt and dnf repositories at `https://tennoworth.app/apt` and `/rpm` are
GPG-signed. If you installed via `apt` or `dnf`, your package manager already
verifies every package and index against this key on every update — there is
no separate checksum step to run.

```
Key:         TennoWorth Packages <pmbaprow@gmail.com>
Fingerprint: CC5F 8E29 7E44 6C5B 8D27  69AC 6409 3BF6 3D57 3CE8
Published:   https://tennoworth.app/tennoworth-archive-keyring.asc
```

Check the key you downloaded matches, before trusting it:

```bash
gpg --show-keys tennoworth-archive-keyring.asc
# Fingerprint must equal the one above, with no spaces:
# CC5F8E297E446C5B8D2769AC64093BF63D573CE8
```

How the key is handled, so you can judge what a compromise would cost:

- The **primary key** only certifies. It has never been on an
  internet-connected server and is not used to sign packages.
- A separate **signing subkey** (`F226 2474 2E2D 5D74`, expires 2028-07-31)
  is the only key material on the server that publishes the repositories.
  If that box were compromised, the subkey can be revoked and rotated without
  users re-importing anything, because the primary they trust is unchanged.
- A revocation certificate exists offline. If you ever see a revocation for
  this key, stop trusting the repositories immediately.

Signing covers the repository, not the identity of the author — it proves a
package came from whoever controls this key and was not altered in transit.
Note this is entirely separate from Windows code signing, which we do **not**
do; see "What we cannot promise".

## How to verify the web app

The production bundle on the deployment is the unmodified output of
`vite build` against the source at the corresponding git commit. To
verify locally:

```bash
git checkout <tag>
cd prototype && bun install --frozen-lockfile && bun run build
diff -r dist/ <deployed dist contents>
```

(`bun.lock` is the source-of-truth lockfile — there is no
`package-lock.json`, so `npm ci` will not work, and an npm-resolved
tree wouldn't reproduce the bun-built `dist/` anyway.)

The web app does not load any third-party scripts. Inspect the
`<head>` of the deployed HTML — the CSP only permits scripts from
the same origin. If you see a `<script src=…>` pointing somewhere
else, the site is compromised.

## Cryptography

The encrypted export feature (`Export inventory`) uses:

- **PBKDF2-HMAC-SHA256** with **600,000 iterations** (OWASP 2023
  recommendation) for key derivation.
- **AES-256-GCM** for encryption, with a fresh 12-byte IV and 16-byte
  salt per export.
- All via the browser's native WebCrypto API. No third-party crypto
  libraries.

The desktop app's on-disk JWT (`wfm-jwt.enc`) uses the same parameters
so one person can audit both.

**Desktop "Remember on this device" (opt-out, default on):** the
desktop app can store the PBKDF2-*derived* unlock key — never the
passphrase itself — in the OS keyring (Secret Service / KWallet /
GNOME Keyring on Linux, Credential Manager on Windows) so listing
unlocks silently after launch, the same protection class your browser
gives the warframe.market cookie. The stored key is salt-bound to the
current `wfm-jwt.enc` (a re-login invalidates it) and useless without
that file. Untick the box, log out, or remove the `tennoworth` entry
in your keyring manager to revert to passphrase-per-session. Trade-off
stated plainly: anything running in your unlocked desktop session that
can read your keyring can combine the two — at-rest offline protection
of the file itself is unchanged.

Source: `prototype/src/lib/crypto.ts`, `companion/wfm-core/src/auth.rs`,
and `companion/tennoworth-desktop/src/keyring_store.rs`.

## Reporting a vulnerability

Open a GitHub issue with the label `security`, **or** email the
maintainer (see the repo's main README for contact). For anything
that could meaningfully harm users (credential theft, RCE in the
desktop app, supply-chain compromise), please do not file a public
issue first — give us a reasonable window to ship a fix.

## Out of scope

- **Cheats / botting / automation that affects gameplay.** This tool
  reads inventory data and posts marketplace orders. It does not
  modify the game, automate gameplay, or interact with anti-cheat
  systems. If that's what you're looking for, this is the wrong
  project.
- **Account recovery if you lose your WFM passphrase.** The encrypted
  export uses a passphrase you choose. If you forget it, the export
  is unrecoverable. By design — we have no way to assist.
