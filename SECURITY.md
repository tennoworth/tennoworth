# Security

This document is the honest threat model. It distinguishes what we
actually commit to from what we can't promise.

## Trust boundaries

The product has three components with three different trust
characteristics:

1. **The web app** (`prototype/`, deployed as static files).
   Pure client-side. No backend. We see no inventory data, no
   credentials, no telemetry. Compromising the static host gives an
   attacker the ability to serve malicious JS to visitors.

2. **The companion CLI** (`companion/`, Rust binary distributed via
   GitHub releases). Runs on the user's machine. Reads the game's
   process memory (Linux: needs `CAP_SYS_PTRACE`; Windows: same-user
   process access). Writes `inventory.json` to disk (0600). For the
   WFM listing feature it holds your WFM JWT encrypted at rest
   (AES-256-GCM, PBKDF2-600k passphrase) and, while `serve` runs,
   relays order operations to warframe.market over a loopback HTTP
   server (`127.0.0.1`, random port, per-process `X-Session-Token`
   auth). The JWT never reaches the browser. If you opt into the
   **AI assistant** (by installing a DeepSeek API key), `serve` also
   relays your questions — with the rows currently shown in your sell
   table (after your filters) as context — to `api.deepseek.com`; that
   key is stored in plaintext at rest (see “The AI assistant” below).

3. **Our build + release pipeline** (GitHub Actions). Four workflows:
   - `refresh-market.yml` — scrapes warframe.market every 2 h and
     commits a static `market.json` + `wfstat-catalog.json` to the
     repo.
   - `release-companion.yml` — on tag push, cross-builds the Rust
     binary for Linux + Windows, generates SHA256SUMS, attaches both
     to a GitHub release.
   - `build-web.yml` — on a push touching `prototype/`, builds the
     static web bundle and publishes it as a rolling `web-latest`
     prerelease asset (the self-host box pulls it with a plain curl).
   - `audit.yml` — on push / PR and weekly, runs dependency advisories
     (`bun audit`, `cargo audit`) plus the JS, Python, and Rust test
     suites.

   Production serving is **not** GitHub-hosted: a self-hosted box (an
   unprivileged LXC, reached only through a Cloudflare Tunnel, fronted
   by Caddy) pulls the CI-built web bundle and runs its own scrape
   timer. That box is a trust boundary the repo's public CI does not
   cover — compromising it would let an attacker serve malicious JS or
   a stale snapshot to visitors.

## What we commit to

- **The web app does not exfiltrate your inventory.** All processing
  is in your browser, and there are **zero third-party origins** in
  the CSP. The only network calls are:
  - `GET /market.json` and `GET /wfstat-catalog.json` from our own
    origin (static files; the item-name catalog used to come from
    warframestat.us directly, but it's baked at build time since
    2026-06)
  - the companion's loopback HTTP server on `127.0.0.1` (only when
    you've connected it; carries the `X-Session-Token` it printed,
    never your WFM credentials)
- **The companion does not transmit your accountId or nonce.** They
  are scraped from game memory and used as URL parameters in a single
  HTTPS request to `api.warframe.com/api/inventory.php`, then
  discarded.
- **Release binaries are reproducibly built in public CI.** You can
  audit the workflow file, the source commit at the tag, and the
  build logs. Each release ships a `SHA256SUMS` file alongside the
  binaries.
- **No telemetry, no analytics, no accounts.** Verify with your
  browser's network tab.

## The AI assistant (optional, off by default)

The in-app AI advisor is the **one feature that sends your data off your
machine**, and it is off unless you opt in. Because it is the single
exception to the app's "your data never leaves the page" promise, here is
exactly what it does:

- **It only runs when you install a DeepSeek API key.** With no key the
  `/assistant` route returns 503 and the drawer stays disabled — no key
  means nothing is ever sent.
- **New egress.** Before the assistant, the companion talked only to
  `127.0.0.1` (the browser) and warframe.market / warframe.com. With a key
  configured, `serve` adds exactly one more destination:
  `https://api.deepseek.com/chat/completions`. The browser still never talks
  to DeepSeek directly — the companion relays, so the API key stays
  server-side and never reaches the page.
- **What's sent.** Your typed question, the recent chat history, and a
  curated context string built from the rows currently shown in your **sell
  table (after your filters)** — for each row: item name, owned/sellable
  counts, average price, 48-hour volume, and vault status (the item list is
  capped to roughly the top 100 rows by sell score) — plus totals across those
  rows (distinct item count, total owned, total estimated plat) and the market
  snapshot's age. Your full inventory, account identifiers, WFM JWT,
  `accountId`, `nonce`, and the companion session token are **never** included.
- **Token-gated like every other companion route.** `/assistant` requires
  the same per-process `X-Session-Token`, plus size caps (question ≤ 2000
  chars, context ≤ 100 KB, history ≤ 12 turns) and a call-rate throttle
  (≤ 20 calls / 60 s → HTTP 429) so a runaway loop or a hostile local client
  can't burn your DeepSeek credit.
- **Prompt-injection surface.** The context is curated WFM / warframestat
  item names plus your own text — not arbitrary third-party content — and the
  system prompt that constrains the model to the data table is
  **server-constructed only**. Client-supplied chat history is sanitized:
  any role other than `user` / `assistant` (notably `system`) is dropped, so
  a client cannot smuggle in its own system instructions.

**The DeepSeek key is stored in plaintext at rest.** It is read from the
`DEEPSEEK_API_KEY` environment variable or, failing that, a `deepseek-key`
file in the same config directory as the encrypted JWT. Unlike the JWT, that
file is **not** encrypted: it is a low-value, easily-rotated API credential
(not an account bearer token), so we deliberately skipped a second
passphrase-unlock flow for it. The companion expects `0600` and logs a
one-line stderr warning (it does **not** fail) if the file is group- or
other-readable. If you'd rather keep no plaintext key on disk, use the
environment variable instead.

## What we cannot promise

- **We cannot promise this is ban-safe.** The companion reads game
  process memory. Equivalent tools (Sainan's `warframe-api-helper`,
  AlecaFrame via Overwolf) have run for years without documented
  bans, but Digital Extremes has never formally blessed the category.
- **We cannot promise warframe.market won't change.** The scraping
  workflow and the listing endpoints (when added) depend on undocumented
  community-API behavior.
- **We cannot promise that a malicious clone of our site doesn't
  exist.** Always verify the URL. Don't enter your WFM credentials
  into anything that isn't the published companion binary.

## How to verify a release

For each Rust companion release on GitHub:

```bash
# After downloading both files from the release page:
sha256sum -c SHA256SUMS
# Should print: wfm-fetch-inventory-linux-x86_64: OK
```

PowerShell equivalent:

```powershell
$expected = (Get-Content SHA256SUMS | Select-String 'windows-x86_64').ToString().Split(' ')[0]
$actual = (Get-FileHash .\wfm-fetch-inventory-windows-x86_64.exe -Algorithm SHA256).Hash.ToLower()
if ($expected -eq $actual) { "OK — checksum matches" } else { "MISMATCH — do NOT run it; re-download" }
```

If it prints `MISMATCH`, the file is corrupt or tampered — delete it and
re-download. Don't run a binary that fails this check.

The `install.sh` and `install.ps1` scripts do this verification
automatically when you use them.

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

The companion's on-disk JWT (`wfm-jwt.enc`) uses the same parameters
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
companion, supply-chain compromise), please do not file a public
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
