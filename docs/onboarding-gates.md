# Onboarding gates — what we ask users to do, and what actually depends on it

**Anchor commit: `c066f7e` (2026-07-25, develop).** Every line number below is
relative to it. If they no longer resolve, run §7 before trusting anything here.

**What this is.** A map of the enforcement surface. For each thing onboarding asks
a user to do, it names the line that *refuses* without it, what stops working, and
how often the ask recurs. The point is that every "this is necessary" claim can be
checked rather than believed.

**What this is not.** Not a punch list. There are no fixes, no priorities, no
severities, and no recommendations here. Its predecessor
(`docs/onboarding-friction.md`, 1518 lines, 77 ranked actions — recover with
`git show 1159f8f^:docs/onboarding-friction.md`) was a punch list, and it was
deleted and gitignored the same day it was committed because the fixes that landed
hours later rotted it on contact.

**Kill condition: if this document ever acquires a "fix" column, delete it.**

---

## 1. Method

**Evidence rules.**

- **R1** — Every necessity verdict cites a *refusal* site: the line that returns an
  error, not a line that merely uses the value. No such line → the verdict reads
  `unestablished`, confidence **U**.
- **R2** — "Enables" lists are closed and carry the grep that produced them. This is
  what makes a claim like *"the JWT is only for WFM writes"* a re-runnable query
  instead of an assertion.
- **R3** — Doc-vs-code disagreements cite both sides adjacently.
- **R4** — Every gate carries a confidence tag: **C** code-verified at the anchor ·
  **R** runtime-verified · **U** inferred, and names its blocker.
- **R5** — Every **U** also gets a line in §6 naming what would settle it.
- **R6** — Any claim about what the *user sees* is **U** or **R**, never **C**.
  Reading code tells you what the process does, not what the human experiences.
- **R7** — One anchor commit, stated above.

**Verdicts, on two orthogonal axes.** Mixing them is how "badly documented" gets
smuggled in as "unnecessary."

*Necessity* — about code. Exactly one per gate.

| Verdict | Falsification test |
|---|---|
| `LOAD-BEARING` | Name the capability and the line that fails without it |
| `CONDITIONALLY LOAD-BEARING` | Name the subpopulation **and** the branch that decides |
| `DEFERRABLE` | Cite the code that already proves it needn't be paid up front |
| `NOT A GATE` | No refusal site exists |

*Framing* — about surfaces. Zero or more. Each needs a doc anchor **and** a
disagreeing code anchor: `MISCONDITIONED` · `UNDOCUMENTED` · `MISPLACED` ·
`DIVERGENT` · `SILENT`.

There is deliberately **no `REMOVABLE`**. That is a design conclusion, and it is the
exact word that turns a diagnosis into a work plan. A cut list is derived on demand:
`grep 'NOT A GATE\|DEFERRABLE' docs/onboarding-gates.md`.

**Out of scope.** No fixes or "should". No effort/priority/severity. No user-facing
copy proposals. No per-surface copy audit — that was most of the predecessor's nine
themes. Nothing after the first ranked sell list renders; the listing half appears
only because gates 3/4/5 sit behind it.

---

## 2. The three paths

There are **three** taught paths, not two. `A-file` and `A-serve` differ on gate 6
as completely as A differs from B.

| Path | What the user is told to do | Taught at |
|---|---|---|
| **A-file** | run `wfm-fetch-inventory`, drop the `inventory.json` it writes | `README.md:59-70`, `App.svelte:1051-1067` |
| **A-serve** | run `wfm-fetch-inventory serve`, browser opens pre-connected | `App.svelte:1016-1050` ("easiest") |
| **B** | install the desktop app, click **Scan inventory** | `README.md:99-112`, `App.svelte:980-1000` |

The README leads with A-file; the SPA labels A-serve "easiest" and demotes the file
to "Prefer a file?". They disagree about which is recommended.

| Gate | A-file | A-serve | B |
|---|---|---|---|
| 2 game running + a call fired | asked | asked | asked |
| 1 ptrace (Linux) | asked (conditionally) | asked (conditionally) | **inherited-unasked** |
| 6 loopback token + LNA | **n/a** | asked | **eliminated** |
| 3 WFM JWT | n/a (no listing) | asked, deferred | asked, deferred |
| 4a passphrase | n/a | asked | asked |
| 4b controlling TTY | n/a | **silent** | eliminated |
| 5 WFM platform | n/a | **silent** (flag default) | asked (dropdown) |
| 7 market.json | not a gate | not a gate | not a gate |

`inherited-unasked` and `eliminated` are opposites and must not be read as the same
empty cell. B *eliminates* gate 6 architecturally; it *inherits* gate 1 with no
surface anywhere.

---

## 3. Gates

Ordered by enforcement depth — how much stops working when the gate is unmet.

### Gate 2 — the game must be running, and a network call must have fired

- **Asks the user to** — start Warframe, get past the title screen, and open the
  trade or profile screen once before scanning. All three paths.
- **Enforced at** — `wfm-core/src/inventory.rs:25-30`, `find_wf_pid()` returns
  `None` → *"Warframe doesn't appear to be running."* And
  `wfm-core/src/scan.rs:106-111`, `if counts.creds.is_empty()` → *"No accountId/nonce
  pair found in WF memory."*
- **Enables** — the entire inventory half: every sell ranking, the tray top-5, the
  notification. Nothing on the WFM listing side.
  `grep -rn 'scan_session\|fetch_inventory_bytes' companion/`
- **Cost if skipped** — the two verbatim errors above. Both actionable.
- **Cadence** — **per pull.** There is no caching anywhere. `InventoryScanner`
  (`inventory.rs:72-74`) holds exactly one field, `scan_lock: Mutex<()>` — a
  single-flight guard, not a cache. `serve.rs:481` calls `state.scanner.scan(None, None)`
  on every `GET /inventory`; desktop does the same via `commands/inventory.rs:36`.
  *Falsifier: if a byte-holding field ever appears in that struct, this is stale.*
- **Conditionality** — none. `matches_warframe()` (`scan.rs:42-58`) handles both the
  Linux 15-char `/proc/<pid>/comm` truncation and the Wine/Proton exe path.
- **Path delta** — same on all three.
- **Asked by** — `install.sh:106-107`, `install.ps1:99-100`, `companion/README.md:37-38`,
  and on B the hero subtitle at `App.svelte:987-990`.
- **Verdict** — `LOAD-BEARING`. Inventory is not obtainable any other way; the
  credentials exist only in the live process.
- **Confidence** — **C**. That the nonce *rotates* within a session is **U**: nothing
  records a TTL, but `pick_dominant` (`scan.rs:105-138`) exists specifically to pick
  the most-frequently-seen pair because stale fragments linger in freed heap.

*Note on framing, not necessity:* this is the only gate that recurs on every single
pull, and the README's step-2 presentation reads as one-time setup.

### Gate 1 — Linux: `CAP_SYS_PTRACE`

- **Asks the user to** — run a `sudo setcap` command they were told to run *only if*
  an error appears.
- **Enforced at** — `scan.rs:153-154`,
  `File::open(&mem_path).map_err(|e| ptrace_open_error(&mem_path, pid, e))`. Opening
  `/proc/<pid>/mem` runs the kernel's `PTRACE_MODE_ATTACH` check; `/proc/<pid>/maps`
  at `:151` does not. Windows equivalent is `OpenProcess` at `scan.rs:267-272`.
- **Enables** — identical set to gate 2. It is gate 2's permission half.
- **Cost if skipped** — `ptrace_open_error()` (`scan.rs:219-250`), which interpolates
  `std::env::current_exe()` at `:226` into a `sudo setcap cap_sys_ptrace=eip "<bin>"`
  line. Reaches the user as CLI stderr, a 503 (`serve.rs:483-487`), or a rejected
  invoke (`commands/inventory.rs:39`).
- **Cadence** — **per binary replacement.** Linux clears file capabilities whenever
  the file is replaced, so every `cargo build --release`, every re-run of
  `install.sh`, and every `pacman -Syu` wipes it. `companion/CLAUDE.md` states this
  as an invariant; `install.sh:125-127` repeats it.
- **Conditionality** — **the real variable is `kernel.yama.ptrace_scope`, not Proton.**
  At scope `0` any same-uid process may attach and the cap is genuinely unnecessary;
  at scope `1` (the common desktop default) only descendants may, and Warframe is a
  child of Steam, not of the companion — so it is required. `scan.rs:240-248`
  special-cases only scope `3`, where setcap does not help and a `sysctl` is needed.
  Four surfaces condition on Proton instead: `install.sh:40`, `:108-114`,
  `InstallWidget.svelte:95-100`, `App.svelte:1041-1047`, and commit `d7fd5cb`
  ("setcap is now on-error guidance everywhere (Proton needs none)"). Proton has no
  bearing on `ptrace_may_access()`. **This machine reports scope `1` and the built
  binary carries no capabilities** — a configuration in which the documented
  "just run it" path fails.
- **Path delta** — A-file and A-serve document it; **B does not, anywhere.**
  `grep -rn 'setcap\|cap_sys' packaging/` → no matches. Neither AUR PKGBUILD sets
  the capability and neither ships an `.install` hook, so a GUI user meets the gate
  as an error string telling them to run `sudo setcap … "/usr/bin/tennoworth-desktop"`,
  wiped again by the next system upgrade. The same message's sudo fallback is not
  usable advice for a Tauri app.
- **Asked by** — `install.sh`, `InstallWidget.svelte`, `App.svelte`, error text. On
  B: **nothing**.
- **Verdict** — `CONDITIONALLY LOAD-BEARING` (population: Linux users with
  `ptrace_scope != 0`; deciding branch is the kernel's, surfaced at `scan.rs:240`)
  + `MISCONDITIONED` + `UNDOCUMENTED` (on B).
- **Confidence** — **C** for the enforcement, **R** for the scope/caps observation
  (§7 reproduces it), **U** for what a scope-1 user actually sees end-to-end.

### Gate 6 — loopback session token + browser Local Network Access

- **Asks the user to** — click **Allow** on a browser permission prompt, and (only
  when the browser didn't auto-open) paste a `http://127.0.0.1:<port>?token=…` line.
- **Enforced at** — token: `serve.rs:461-472`, constant-time compare via
  `subtle::ConstantTimeEq`, → 401 `{"error": "missing or invalid X-Session-Token"}`
  on every route except `OPTIONS` and `/health`. LNA: not enforced by us — the
  browser enforces it; we opt in with `targetAddressSpace: 'loopback'` at
  `companion-transport.ts:20` and `companion.ts:126`.
- **Enables** — every companion HTTP call on A-serve, inventory included.
  `grep -rn 'X-Session-Token' prototype/src companion/`
- **Cost if skipped** — bad token → clean 401. Denied LNA → the app's cross-view
  recovery banner (`App.svelte:1932-1957`), sharpened by a Permissions-API probe at
  `companion-connect.ts:60-73`. **Undecided** LNA is the pathological case: per
  `companion.ts:104-114`, a fetch to 127.0.0.1 *hangs indefinitely* — neither
  resolves nor rejects — so without a timeout the connect flow never reaches a
  failure state. Defended by `HEALTH_TIMEOUT_MS = 8000` /
  `PENDING_PLAN_TIMEOUT_MS = 10000` (`companion.ts:115-116`).
- **Cadence** — token per `serve` process (rotates every run). LNA grant per
  browser+origin.
- **Conditionality** — browser-conditional, not OS-conditional: Chromium, and
  Firefox 149+ under ETP Strict / 151+ for everyone.
- **Path delta** — **A-file never touches it** (no server, no token). **B eliminates
  it**: the webview is same-origin `tauri://localhost`, `createTransport()`
  (`transport.ts:428-430`) selects the `invoke` path, and no HTTP server or token
  exists at all — `WfmSession` has no token field. The desktop CSP drops the loopback
  entries entirely.
- **Asked by** — `serve.rs:120-123` startup banner, `App.svelte:1932-1957` recovery.
- **Verdict** — `LOAD-BEARING` on A-serve; `NOT A GATE` on A-file and B.
- **Confidence** — **C** for token and opt-in. The hang is **U** — the code comment
  documents it, but it is a browser behaviour not reproduced here.

*This is the highest-friction gate in the product and the one we control least. It
is also the single strongest argument for path B.*

### Gate 3 — the warframe.market JWT

- **Asks the user to** — run `login` once (A), or fill the login dialog (B).
- **Enforced at** — `serve.rs:292` `ensure_unlocked()`, reached from six route sites:
  `:512` `POST /plan/resume`, `:542` `POST /plan`, `:561` `GET /orders`, `:578`
  `POST /orders/visibility`, `:593` `DELETE /order/<id>`, `:618` `PATCH /order/<id>`.
  (`:182` is the eager `--passphrase-stdin` fail-fast, not a route.) Desktop mirror:
  `wfm_session.rs:179-188` `require_unlocked()`.
- **Enables** — order create (`plan.rs:436`), order patch/delete and `GET /orders`
  (`listing.rs:103/160/189`), and `fetch_wfm_me` (`auth.rs:250-268`). That is the
  closed list.
  `grep -rn 'unlocked.jwt' companion/wfm-core/` → exactly those four sites.
  **`grep -c jwt companion/wfm-core/src/inventory.rs` → `0`.** The inventory path
  never touches it; `inventory.rs:15-16` says so, and `serve.rs:474-477` repeats it.
- **Cost if skipped** — 401 `{needs_login: true}` (`serve.rs:717-725`); desktop gets
  a typed `needs_login`. Reading market data needs it either: `catalog.rs:31` uses
  plain `wfm_headers`, and the whole scraper is anonymous.
- **Cadence** — once ever. A `login` that lands while `serve` is already running is
  picked up with no restart (`late_load_locked()`, `serve.rs:338-357`; unit tests at
  `:811-847`).
- **Conditionality** — none, but entirely optional: a user who never lists never
  needs it.
- **Path delta** — same state machine on A-serve and B. n/a on A-file.
- **Asked by** — `App.svelte:1069-1086` (step 04, marked optional),
  `README.md:73-77`, `serve.rs:130-134`.
- **Verdict** — `DEFERRABLE`. The code already proves it: `run_serve` deliberately
  *peeks* the envelope without decrypting (`serve.rs:70-103`), starts fine with no
  login file at all (`ListingAuth::Unavailable`, `:102`), and unlocks lazily.
- **Confidence** — **C** for the routing. The live 401/503 behaviour against a real
  WFM account is **U** — this is the open item already recorded in `CLAUDE.md`.

### Gate 4a — the encryption passphrase

- **Asks the user to** — invent and retype a ≥12-character passphrase, distinct from
  their WFM password.
- **Enforced at** — `wfm-fetch-inventory/src/main.rs:230` `if passphrase.len() < 12`
  and `tennoworth-desktop/src/wfm_session.rs:316`
  `if passphrase.chars().count() < 12`. Decryption refusal: `auth.rs:236`,
  *"Wrong passphrase, or the JWT file was modified."*
- **Enables** — decrypting the JWT, hence everything in gate 3.
- **Cost if skipped** — can't be skipped; it is a precondition of gate 3 existing.
- **Cadence** — **per `serve` process** on A (prompted lazily at the first listing,
  `serve.rs:394-405`, three attempts). **Effectively once ever** on B: `keyring_store.rs`
  stores the *PBKDF2-derived key*, never the passphrase, and `try_silent_unlock()`
  (`wfm_session.rs:259-292`) runs before the modal. The stored key is salt-bound, so
  a re-login invalidates it detectably (`auth.rs:297-306`).
- **Conditionality** — keyring backend differs by OS; the gate does not.
- **Path delta** — was `DIVERGENT`; **fixed 2026-08-01, after the anchor commit.**
  `main.rs:230` counted **bytes** while `wfm_session.rs:316` counted **chars**, under
  a comment reading *"Same floor as the CLI `login`"* — so a 4-character CJK
  passphrase (12 bytes) was accepted by A and rejected by B. Both callers now share
  `wfm_core::auth::validate_passphrase()`, which counts characters, the unit the
  error message promises. Deduplicated rather than fixture-tested: both sides are
  Rust and both already depend on wfm-core, so the floor cannot drift again.
  Regression pinned by `auth::tests::passphrase_floor_counts_characters_not_bytes`.
- **Asked by** — `main.rs:222-232` (CLI), `WfmAuthDialogs.svelte:143-157` (B).
- **Verdict** — `LOAD-BEARING` + `DIVERGENT`. The threat model is stated and
  calibrated: the JWT is a multi-month bearer credential (`SECURITY.md:20-25`) and is
  AES-256-GCM/PBKDF2-600k encrypted, while the DeepSeek key is deliberately plaintext
  (`SECURITY.md:111-119`) — so this is a considered choice, not blanket paranoia.
- **Confidence** — **C**.

### Gate 4b — `serve` needs a controlling TTY

- **Asks the user to** — nothing. It is never asked; it is discovered.
- **Enforced at** — `serve.rs:388-393`, inside `build_unlocked`'s `Tty` branch:
  *"Listing needs your passphrase, but serve has no interactive terminal."* Also
  `serve.rs:365` for the late-login case.
- **Enables** — the passphrase prompt, hence gate 4a, hence gate 3.
- **Cost if skipped** — the bail above. The comment at `:385-387` records why it
  exists: to replace rpassword's cryptic `os-error-6`.
- **Cadence** — per `serve` invocation.
- **Conditionality** — context-conditional: bites IDE run buttons, `nohup`, systemd
  units. Escape hatch `--passphrase-stdin` (`serve.rs:87-98`), which has its own
  sharp edge — a login arriving after a `--passphrase-stdin` serve started with no
  login file cannot be unlocked, and returns an actionable "restart serve"
  (`serve.rs:367-372`).
- **Path delta** — A-serve only. **B eliminates it** — the passphrase arrives from
  the webview, so there is no terminal in the design.
- **Asked by** — **nothing**, until it fires. `README.md:80-82` mentions a TTY
  requirement but attaches it to startup, which has not been true since lazy unlock.
- **Verdict** — `CONDITIONALLY LOAD-BEARING` (population: non-TTY launch contexts;
  branch at `serve.rs:388`) + `UNDOCUMENTED`.
- **Confidence** — **C**.

### Gate 5 — WFM account platform

- **Asks the user to** — pick pc / ps4 / xbox / switch (B), or nothing at all (A,
  where it is a flag defaulting to `pc`).
- **Enforced at** — `wfm-client/src/lib.rs:67-76` `validate_platform`, called at
  `main.rs:189` and `wfm_session.rs:307`. It rejects values *outside* the four-item
  list — nothing more.
- **Enables** — the `Platform:` header on every WFM call (`wfm-client/src/lib.rs:34-42`),
  i.e. targeting the right market.
- **Cost if skipped** — **it cannot be skipped, only defaulted, and a wrong-but-valid
  value fails silently.** `validate_platform` cannot detect it. The value is then
  baked immutably into the encrypted envelope (`auth.rs:180`, `:198`) and every later
  read is one-way (`serve.rs:83-87`, `wfm_session.rs:222-230`). There is no
  re-platform path short of re-running `login`.
- **Cadence** — once, at login. **Immutable thereafter.**
- **Conditionality** — a WFM-account property, not an OS property. Irrelevant to the
  ~PC majority; consequential for console users.
- **Path delta** — **opposite visibility, same consequence.** B *asks* (a visible
  `<select>`, `WfmAuthDialogs.svelte:134-142`); A *defaults invisibly* behind
  `--platform` (`main.rs:109-113`). The CLI has no interactive prompt.
- **Asked by** — B's dialog. On A: **nothing** — only a flag in the help text.
- **Verdict** — `LOAD-BEARING` + `SILENT` (on A).
- **Confidence** — **C** for the mechanism; **U** for the console failure itself.

*Cross-reference for §4:* `ct` — DE's own platform tag — **is** scraped from memory
(`scan.rs:75-76`, defaulting to `"STM"` at `:129`) and is never consulted when
choosing the WFM platform.

---

## 4. Cross-gate interactions

Things no single gate entry can show.

**4b × 6 — the permanently hung browser.** `companion.ts:110-114` states that listing
routes deliberately get *no* timeout, because the first listing call legitimately
blocks while `serve` prompts for a passphrase on its own terminal. That is correct
design given gate 4b. But the user is looking at a browser, and the prompt is in a
terminal window they were told they could ignore after step 02. Neither gate's entry
can surface this; it lives in the seam.

**1 × 3 — cadence mismatch.** The ptrace capability is wiped by every binary
replacement; the encrypted JWT survives upgrades untouched. After an AUR rebuild a
user therefore has working credentials and a broken scan — the half of the product
that needs no login is the half that breaks.

**5 × 2 — the strongest signal sits next to the weakest guess.** Gate 2 already
scrapes `ct` from game memory. Gate 5 separately guesses `pc`. `NS` → `switch` is
unambiguous, and nothing connects them. Note `Crossplay: true` is sent on every
request (`wfm-client/src/lib.rs:39`), which softens but does not remove the
consequence.

---

## 5. Asked, but not gates

- **`market.json`** — the user does nothing. Three layers: hosted same-origin fetch
  (`market.ts:7,13`); a compile-time `include_str!` floor for desktop
  (`sellables.rs:35-36`); and an ETag-conditional background refresh in Rust, never
  the webview (`market.rs:120-227`). Every failure path degrades to `keep_cache`.
  `NOT A GATE`, and the part of onboarding with no failure state that leaves the user
  with nothing.
- **Install-script checksum verification** — genuinely hard-fails
  (`install.sh:66-86`), but it is the installer refusing, not a user step.
- **Windows SmartScreen** — a real wall on unsigned artifacts. `MISPLACED`:
  `README.md:101-102` points the user at `SECURITY.md`, and
  `grep -ci smartscreen SECURITY.md` → **0** while `docs/signing-runbook.md` → **6**.
  The material exists; the pointer goes to the wrong file.
- **Linux desktop means building from AUR source** — no AppImage/deb/rpm by decision
  (`README.md:108-112`). A substantial Linux-only install step, but not an
  enforcement gate.

---

## 6. Unverified register

| Claim | Blocker | What would settle it |
|---|---|---|
| Gate 5 silently mis-targets for console users | No console WFM account | A `ps4`/`switch` login, then observe `/v2/me` |
| Firefox undecided-LNA hang (`companion.ts:104-114`) | No FF 149+ / ETP Strict here | Load the SPA in that config, leave the prompt undismissed |
| Gate 3 live 401/503 branches | No live WFM login — the open item already in `CLAUDE.md` | Real `login`, then hit a listing route mid-unlock |
| Nonce rotation within a session | Nothing records a TTL | Two scans minutes apart, compare nonces |
| What a scope-1 Linux user sees end-to-end | Requires the game running | Run the scan with no capability set |
| SmartScreen wording on the desktop installer | No clean Windows box | Fresh VM, run the `desktop-latest` `.exe` |

Per R6, every "what the user sees" row here is correctly **U**, not **C**.

---

## 7. Staleness self-check

Run this block. If the outputs no longer match the comments, this document is stale.

```sh
# Gate 1 — ptrace is scope-conditional, not Proton-conditional
cat /proc/sys/kernel/yama/ptrace_scope                    # 1 (on this machine)
getcap companion/target/release/wfm-fetch-inventory       # empty
grep -rn 'setcap\|cap_sys' packaging/                     # no matches (gate 1 unasked on B)

# Gate 2 — recurring, uncached
grep -n 'scan_lock' companion/wfm-core/src/inventory.rs   # Mutex<()> only, stores no bytes

# Gate 3 — the closed list
grep -c 'jwt' companion/wfm-core/src/inventory.rs         # 0  <- the proof
grep -n 'ensure_unlocked' companion/wfm-fetch-inventory/src/serve.rs   # routes 512 542 561 578 593 618
grep -rn 'unlocked.jwt' companion/wfm-core/               # listing.rs 103/160/189 + plan.rs 436

# Gate 4a/4b — one shared validator; two call sites, zero local floors
grep -rn 'validate_passphrase' companion/ --include='*.rs' # auth.rs def + main.rs + wfm_session.rs
grep -rn 'chars().count() < 12\|passphrase.len() < 12' companion/ --include='*.rs'  # no matches
grep -rn 'is_terminal' companion/ --include='*.rs'        # serve.rs 365, 388

# Gate 5
grep -n 'pub const PLATFORMS' companion/wfm-client/src/lib.rs               # 62
grep -n 'platform' companion/wfm-fetch-inventory/src/main.rs | head         # 109-113 flag + default

# Gate 6
# two real call sites; the other matches are a comment, the RequestInit type decl, and tests
grep -rn "targetAddressSpace: 'loopback'" prototype/src/lib/compani*.ts     # -transport.ts:20, .ts:126
grep -n 'X-Session-Token' companion/wfm-fetch-inventory/src/serve.rs        # 463

# Path B has no onboarding surface
grep -c 'isDesktop' prototype/src/App.svelte              # 27
grep -n 'isDesktop' prototype/src/App.svelte              # structural: 780 1007 1120 1619

# Misplaced SmartScreen pointer
grep -ci smartscreen SECURITY.md docs/signing-runbook.md  # 0 and 6

# Prior art
git show 1159f8f^:docs/onboarding-friction.md | wc -l     # 1518
```
