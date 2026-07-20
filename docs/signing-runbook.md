# Code-signing runbook

Two independent signing systems live here: **Windows Authenticode** for the
companion CLI (§1–8, the Phase B2 pipeline) and the **Tauri updater keypair**
for the desktop app's auto-update (§9–10, Phase C5). They share nothing — a
Certum cert cannot sign updater bundles and the updater key means nothing to
SmartScreen — but both follow the same rule: private key material never
touches the repo.

## Windows / Authenticode (companion CLI)

Operational guide for the Phase B2 signing pipeline. This is the
**cert-independent prep**: the workflow (`.github/workflows/release-companion.yml`)
and the installer (`prototype/public/install.ps1`) already know how to sign,
verify, and scan — they are gated off until the secrets below exist. When the
Certum certificate arrives, turning signing on should be a **secrets-only
change**: no workflow edits for the PFX path, one small edit for the SimplySign
path (documented under "SimplySign in CI").

Until then every release ships **unsigned but SHA-256-verified**, which is the
state the repo is committed to right now (see `SECURITY.md`).

---

## 1. What to buy

- **Product:** Certum **Open Source Code Signing** certificate (OV class,
  issued to an individual open-source developer). ~€70/yr; Certum runs frequent
  discounts through resellers.
- **Why Certum, not Azure Trusted Signing:** Azure Trusted Signing is not
  available to EU individual developers (2026 preview restriction — see the
  product plan's "Rejected ideas"). Certum's Open Source program is the standard
  route for solo EU maintainers and is the one certificate class priced for it.
- **Eligibility requirements:**
  - **Public source repository** — the project must be open source. We qualify
    (`tennoworth/tennoworth` is public).
  - **Identity validation** of the individual. Certum verifies a real person:
    government photo ID, and typically a video call or notarised documents.
    Budget **days to a couple of weeks** of elapsed time — this is why the
    product plan says "buy the cert now" (Phase A4) even before code needs it.
  - EU residency for the individual Open Source tier.
- **What you receive:** an OV code-signing credential. Since the June-2023
  CA/Browser Forum baseline, the **private key must live on hardware** (a
  FIPS-140-2 token or a cloud HSM). Certum delivers this one of two ways —
  see the decision point in §3.

---

## 2. Secrets the workflow expects

Configure these under **repo → Settings → Secrets and variables → Actions →
Repository secrets**. Names are load-bearing — they must match the `env:` blocks
in `release-companion.yml` exactly. **Nothing is configured today; that is the
shipping state.**

### Windows signing — Path 1: portable PFX

| Secret | Value |
|---|---|
| `WINDOWS_PFX_BASE64` | The `.pfx` (PKCS#12) file, base64-encoded. On Linux: `base64 -w0 cert.pfx`. On Windows PowerShell: `[Convert]::ToBase64String([IO.File]::ReadAllBytes('cert.pfx'))` |
| `WINDOWS_PFX_PASSWORD` | The PFX export password |

Setting `WINDOWS_PFX_BASE64` (non-empty) flips signing on and selects the PFX
path. **This path realistically only applies to a test/legacy cert** — see §3
for why a new Certum OV cert probably cannot produce an exportable PFX.

### Windows signing — Path 2: Certum SimplySign (cloud HSM)

| Secret | Value |
|---|---|
| `SIMPLYSIGN_ENABLED` | Set to `true` to select the SimplySign path |
| `SIMPLYSIGN_USER` | SimplySign account / login |
| `SIMPLYSIGN_PASSWORD` | SimplySign account password |
| `SIMPLYSIGN_OTP_SECRET` | TOTP seed for headless OTP generation (see "SimplySign in CI") |
| `SIMPLYSIGN_CERT_THUMBPRINT` | Which credential in the cloud keystore to sign with |

`SIMPLYSIGN_ENABLED=true` selects Path 2; the rest feed the (currently
pluggable) signing command. The exact set may change once the concrete tool is
chosen — keep this table and the workflow `env:` block in lockstep.

### VirusTotal (any release, both binaries)

| Secret | Value |
|---|---|
| `VT_API_KEY` | A VirusTotal API key (free "community" tier is enough — 4 req/min, 500/day; a release submits 2 files) |

Absent → the scan step skips and writes a "VirusTotal skipped" notice to the job
summary. The release still publishes.

---

## 3. Decision point: SimplySign vs. PFX

This is the one real fork, and it depends entirely on **how Certum delivers the
key**. Determine which you have on day one — it dictates whether enabling
signing is truly secrets-only.

### If you get a portable PFX (Path 1)

Possible only for older organisation certs or an explicitly soft/test cert.
**Do not expect this for a new individual OV cert** — the CA/B hardware-key
rule means the key is generated on and never leaves the token/HSM, so there is
nothing to export into a `.pfx`.

- **What changes in the workflow:** nothing. Set `WINDOWS_PFX_BASE64` +
  `WINDOWS_PFX_PASSWORD` and the existing `signtool sign /f <pfx> /p <pw> /tr
  http://time.certum.pl /td SHA256 /fd SHA256` step runs as-is.
- Genuinely a secrets-only change.

### If you get SimplySign (Path 2) — the likely reality

Certum's modern delivery is **SimplySign**: the credential lives in a cloud
QSCD/HSM, accessed through the **SimplySign Desktop** app which exposes it as a
virtual smartcard via **PKCS#11**, authenticated with username/password **plus
a TOTP** from the SimplySign mobile app. (The physical variant is a Certum
cryptographic USB card — same PKCS#11 story, but needs a machine with the card
plugged in, i.e. a self-hosted runner.)

- **What changes in the workflow:** the `Sign Windows binary` step currently
  **throws** on the SimplySign branch — it is a clearly-marked pluggable block.
  Wire the concrete signing command there (see below), then set
  `SIMPLYSIGN_ENABLED=true` and the other `SIMPLYSIGN_*` secrets.
- The `Verify Authenticode signature` step needs **no change** either way:
  `signtool verify /pa` checks the resulting Authenticode signature regardless
  of how it was produced.

#### SimplySign in CI (the honest part)

Automating SimplySign headlessly is the hard bit, because of the OTP. Options,
roughly in order of robustness:

1. **Self-hosted Windows runner** with SimplySign Desktop installed and logged
   in (session kept alive). CI calls `signtool sign /sha1 <thumbprint> /tr
   http://time.certum.pl /td SHA256 /fd SHA256 <asset>` against the PKCS#11
   virtual card. Most reliable; costs you a maintained box.
2. **Hosted runner + programmatic OTP.** If Certum lets you capture the TOTP
   seed when enrolling the authenticator, store it as `SIMPLYSIGN_OTP_SECRET`,
   generate the 6-digit code in the step (`oathtool --totp -b $SECRET` or an
   equivalent), drive SimplySign Desktop's CLI login, then sign via signtool or
   `osslsigncode` against the PKCS#11 engine. Fragile; depends on Certum's tools
   staying scriptable.
3. **`osslsigncode` via PKCS#11 engine** — works cross-platform and can run on
   the Linux publish leg, but still needs the SimplySign PKCS#11 module +
   authenticated session, so it does not dodge the OTP problem.

Pick one, replace the `throw` in the SimplySign branch with the real command
(the `SIMPLYSIGN_*` env vars are already in scope there), and document which you
chose in a comment next to it. Do **not** silently ship an unsigned binary from
that branch — the deliberate `throw` prevents a "secrets set but nothing signed"
false-positive that the verify step could not catch (verify only runs when the
sign step reports success).

---

## 4. Timestamping

Always sign **with** an RFC-3161 timestamp (`/tr http://time.certum.pl /td
SHA256` — already in the workflow). A timestamp binds "this was signed while the
cert was valid", so the signature keeps verifying **after the cert expires**.
Without it, every signed binary goes bad the day the cert lapses. If the issuing
TSA URL differs from Certum's, update the `$ts` value in the sign step.

---

## 5. Key rotation & recovery

- **Backup (PFX path only):** if you ever hold an exportable PFX, keep an
  offline encrypted copy of it **and** the password in a password manager.
  Losing it means re-validating identity for a fresh cert.
- **Hardware/SimplySign path:** the key is not exportable by design — there is
  nothing to back up and nothing that can leak as a file. "Recovery" means
  re-enrolling with Certum (which re-runs identity validation). Keep the
  SimplySign account credentials + the authenticator seed backed up; losing the
  authenticator is the real outage risk, not the key.
- **Rotation:** replace the relevant secrets (`WINDOWS_PFX_BASE64` +
  password, or the `SIMPLYSIGN_*` set) and cut a new release. Publisher
  reputation (§7) is tied to the **subject identity**, not the specific cert
  serial, so renewing/rotating a cert for the same individual does **not** reset
  SmartScreen reputation.
- **Compromise:** if a PFX + password is believed exposed, contact Certum to
  **revoke** immediately, rotate the secret, and re-release. Timestamped
  binaries signed before the revocation date remain valid; anything after the
  revocation instant fails. (Another reason to prefer the non-exportable
  SimplySign path — there is no file to exfiltrate.)
- **Never** commit a cert, PFX, or password to the repo, and never `echo` a
  secret in a workflow step. The workflow only ever references them through the
  `env:` indirection.

---

## 6. AV false positives & VirusTotal policy

A memory-reading exe (`ReadProcessMemory` / `/proc/<pid>/mem`) trips heuristic
AV engines — this is risk-register item #3, expected, not a surprise.

- **VirusTotal monitoring:** every release, once `VT_API_KEY` is set, the
  `Scan release artifacts on VirusTotal` step uploads both binaries and drops
  the per-file GUI links into the job summary. **Policy: check those links after
  each release.** A couple of no-name engines flagging is normal; a
  mainstream engine (Microsoft Defender, Kaspersky, ESET, BitDefender) flagging
  is worth acting on. The link is keyed on the file SHA-256, so it stays valid
  and re-checkable over time (re-scans as engines update).
- **Microsoft false-positive submission** (the one that matters most, since
  Defender ships on every Windows box):
  1. Go to the Microsoft Security Intelligence submission portal
     (`microsoft.com/wdsi/filesubmission`), sign in, choose **"Software
     developer"** and **"Incorrectly detected as malware/PUA"**.
  2. Upload the exact released binary (or give the download URL), cite the
     VirusTotal link, and describe what the tool does (local memory scan of
     Warframe to read inventory; open-source; CI-built; link the repo and
     `SECURITY.md`).
  3. Microsoft usually responds in 1–3 business days; a corrected verdict
     propagates to Defender via cloud definitions without a client update.
  4. **Signing helps here:** a consistent signed publisher identity makes
     "this is a known developer, not malware" a far easier argument, and
     lets Microsoft whitelist by publisher rather than per-file-hash.
- Submit the equivalent false-positive report to any other mainstream engine
  that flags (most have a developer dispute form).
- **Never auto-elevate** to work around AV, and never tell users to add blanket
  AV exclusions — say plainly in the trust page what the tool reads and why.

---

## 7. SmartScreen reputation reality

Signing is necessary but **not instantly sufficient**. Set expectations
honestly (this is risk-register item #2):

- **Signed-but-new still warns.** A brand-new cert with no download history
  still triggers SmartScreen's "Windows protected your PC" / "unrecognized app"
  prompt. Signing gives you a *stable identity to accrue reputation against*; it
  does not skip the ramp.
- **Reputation is per-publisher and accrues with clean installs over time.**
  There is no fee to buy it off (that was EV certs' old instant-reputation
  perk, and even that is gone under current rules). Expect the warning to fade
  over **weeks of real download volume**, not on day one.
- **It carries across files.** Reputation attaches to the publisher identity,
  so signing today's CLI (`wfm-fetch-inventory.exe`) is not wasted when the
  Tauri desktop exe ships later under the same identity — they pool reputation.
- **Measure it.** Per the product plan's Phase B gate, run a **clean Win11 VM
  first-run test each release** and record what SmartScreen actually shows. That
  is the ground truth, not what we hope signing bought us.
- **User-facing copy** already reflects this: `install.ps1` prints a
  reputation-ramp note when the binary is signed, and the honest "not signed
  yet + checksum was verified" note while it is not. Keep the winget listing +
  install education aligned with whatever the VM test shows.

---

## 8. What the pipeline does right now (no secrets)

For reference, so you can confirm the shipping state is behaving:

- **Build / Windows leg:** sign step skipped → verify skipped → "Windows binary
  UNSIGNED" notice written to the job summary + a `::notice::` annotation.
- **Build / Linux leg:** no signing steps apply (Windows-only); no notice.
- **Publish:** SHA256SUMS generated as before; VirusTotal step skipped →
  "VirusTotal skipped" notice; GitHub release created with both binaries +
  `SHA256SUMS`.
- **install.ps1:** SHA-256 verified (hard requirement, unchanged); Authenticode
  status reported as advisory (`not present yet`); install never blocked on
  signature; honest unsigned/SmartScreen note printed.

Flipping any secret on changes only the corresponding gated step — nothing else
in the release flow moves.

---

## 9. Tauri updater keypair (desktop auto-update, Phase C5)

The desktop app (`companion/tennoworth-desktop`) checks
`https://github.com/tennoworth/tennoworth/releases/latest/download/latest.json`
at launch and on demand. Every update bundle is signed with a **minisign
keypair** and verified against the public key baked into
`tauri.conf.json → plugins.updater.pubkey` before install. This key is the
whole trust model: whoever holds the private key can ship code to every
desktop install, so it is generated **offline, by you, once** — never in CI,
never by an agent, never committed.

### Current state (deliberate)

`plugins.updater.pubkey` holds a clearly-marked **placeholder**. Until you
replace it, signature verification fails on everything, so no update can ever
install — checks still work and degrade to "no update". This is the safe
shipping state until the keypair exists.

### Generate the keypair (offline, one time)

```sh
# tauri-cli (cargo install tauri-cli --version '^2' — or use `bunx @tauri-apps/cli`)
cargo tauri signer generate -w ~/.tauri/tennoworth-updater.key
```

- Set a **password** when prompted (it encrypts the private key at rest;
  CI needs it too, see below).
- Output: `tennoworth-updater.key` (PRIVATE — the secret) and
  `tennoworth-updater.key.pub` (public, safe to publish).
- Paste the **contents of the `.pub` file** (the base64 blob, one line) into
  `companion/tennoworth-desktop/tauri.conf.json → plugins.updater.pubkey`
  and commit that — the pubkey is meant to be public.

### Where the private key lives

- Your machine (`~/.tauri/`), **plus two offline backups** (password manager
  attachment + a drive that is not this computer). §"Loss" explains why two.
- The password lives in the password manager, separate from the key file.
- Never in the repo, never echoed in a workflow, never pasted to an agent.

### How CI signs (when the C8 release workflow lands)

`tauri build` signs updater artifacts automatically when these env vars are
present (set as GitHub Actions **repository secrets**, same indirection
pattern as §2):

| Secret | Value |
|---|---|
| `TAURI_SIGNING_PRIVATE_KEY` | Contents of `tennoworth-updater.key` (the base64 blob — the file content itself, not a path) |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | The password set at generation |

`bundle.createUpdaterArtifacts: true` is already set in `tauri.conf.json`, so
a signed build emits, per platform, the bundle **plus a detached `.sig` file**
whose contents go into `latest.json` (§10). Without the secrets, `tauri build`
still produces installable bundles — they just can't be consumed by the
updater.

### Rotation

Installed apps verify with the **old** pubkey until they update. Order
matters:

1. Generate the new keypair offline (as above).
2. Replace `plugins.updater.pubkey` with the **new** pubkey in the repo.
3. Cut that release signed with the **OLD** key (leave the old
   `TAURI_SIGNING_PRIVATE_KEY` secret in place for this one release) — this is
   the bridge release: old installs accept it, and it carries the new pubkey.
4. After it ships, swap the CI secrets to the new key. All later releases are
   signed with the new key, which the bridge release taught installs to trust.

Skipping step 3 strands every existing install (they'd reject the first
new-key release).

### Loss

There is no CA and no re-issue: lose the private key (and both backups) and
**existing installs can never auto-update again** — their baked-in pubkey will
reject anything you can still produce. Recovery is manual: generate a fresh
pair, ship a new release, and tell users to download and reinstall once (the
trust page + release notes). Painful but bounded — this is why the two offline
backups are non-negotiable.

### Compromise

An attacker needs the private key **and** its password **and** the ability to
serve a manifest (GitHub release write access) to push a malicious update. If
you believe the key leaked:

1. Delete/rotate the GitHub Actions secrets immediately.
2. Run the rotation flow above (the bridge release is safe to sign with a
   compromised-but-not-yet-abused key; if abuse is confirmed, fall back to the
   manual-reinstall path instead and say so loudly).
3. Delete any GitHub release assets the attacker touched; publish an advisory
   in the repo + trust page.

Timestamping does not apply here (minisign, not Authenticode) — there is
nothing that keeps old signatures valid across rotation except the bridge
release.

---

## 10. What the desktop release workflow must produce (C8 contract)

There is **no desktop release workflow yet** — C5 deliberately shipped only
the client side. The updater endpoint 404s today (and the placeholder pubkey
rejects everything), which the desktop app treats as "no update available".
When C8 builds the packaging workflow, the updater side of it must produce,
per release:

1. **Signed bundles + detached signatures**, built by `tauri build` with the
   §9 secrets set: at minimum `TennoWorth_<ver>_amd64.AppImage` (+ `.sig`) on
   the Linux leg and the NSIS `TennoWorth_<ver>_x64-setup.exe` (+ `.sig`) on
   the Windows leg. The `.sig` files exist only when `TAURI_SIGNING_*` is
   configured — the workflow should **fail loudly** if they're missing rather
   than publish an un-updatable release (mirror the SimplySign `throw`
   pattern, §3).
2. **`latest.json`**, uploaded as a release asset alongside the bundles:

   ```json
   {
     "version": "0.2.0",
     "notes": "…release notes…",
     "pub_date": "2026-08-01T12:00:00Z",
     "platforms": {
       "linux-x86_64": {
         "signature": "<contents of the .AppImage.sig file>",
         "url": "https://github.com/tennoworth/tennoworth/releases/download/desktop-v0.2.0/TennoWorth_0.2.0_amd64.AppImage"
       },
       "windows-x86_64": {
         "signature": "<contents of the -setup.exe.sig file>",
         "url": "https://github.com/tennoworth/tennoworth/releases/download/desktop-v0.2.0/TennoWorth_0.2.0_x64-setup.exe"
       }
     }
   }
   ```

   `signature` is the **contents** of the `.sig` file, not a URL to it.
   `version` must be plain semver (no leading `v`) and strictly greater than
   the installed version for the updater to offer it.
3. **The "latest release" gotcha:** the endpoint uses
   `releases/latest/download/latest.json`, and this repo also cuts companion
   CLI releases (`v*` tags) that contain no `latest.json`. GitHub's "latest"
   is the newest non-draft, non-prerelease release of the whole repo — so a
   CLI release published after a desktop release makes the endpoint 404 until
   the next desktop release (checks degrade to "no update"; nothing breaks,
   but updates stall). C8 must pick one: mark CLI releases as pre-releases,
   re-attach `latest.json` to every release, or move the endpoint to a fixed
   tag (e.g. `releases/download/desktop-latest/latest.json`) that the desktop
   workflow force-updates. Decide there — the config change is one line.
4. **Updater behavior** the workflow can rely on: Linux updates only apply to
   the AppImage packaging (a raw binary or distro package refuses to install
   updates — expected); Windows runs the NSIS installer in passive mode, which
   restarts the app itself. Neither downloads anything without the user
   clicking Install in the banner.
