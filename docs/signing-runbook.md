# Code-signing runbook (Windows / Authenticode)

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
