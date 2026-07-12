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
   auth). The JWT never reaches the browser.

3. **Our build + release pipeline** (GitHub Actions). Two workflows:
   - `refresh-market.yml` — scrapes warframe.market every 2 h and
     commits a static `market.json` + `wfstat-catalog.json` to the
     repo.
   - `release-companion.yml` — on tag push, cross-builds the Rust
     binary for Linux + Windows, generates SHA256SUMS, attaches both
     to a GitHub release.

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

Source: `prototype/src/lib/crypto.ts` and `companion/src/main.rs`.

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
