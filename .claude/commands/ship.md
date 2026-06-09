---
description: Drive toward a polished, public v1 — correctness, trust, distribution, onboarding, and the sell-score differentiator. One focused chunk per fire; stop when v1 is reached.
---

# Goal

Ship a **polished, public v1** of this product. The end-state:

> A stranger on **Windows or Linux** can install the companion (one
> `setcap`, no recurring sudo), pull their inventory, load it into a
> **properly-hosted** web app, and get **correct, non-empty** sell
> guidance in every view — with a **clean security posture**, a
> frictionless **first run**, and **checksummed binaries** from a named
> GitHub repo. The "measurably better at what to sell right now" claim
> must actually hold.

Work the tiers **in order**. A later tier never starts while an earlier
one has open items.

## Tier 1 · Correctness — nothing visibly broken

1. **Baro view CORS bug.** `App.svelte` fetches
   `https://api.warframestat.us/.../voidTrader/` at runtime → blocked by
   CORS (warframestat sends no `Access-Control-Allow-Origin`).
   **Default fix:** bake Baro data into `market.json` at build time in
   `scripts/csv_to_market_json.py` — mirror the `relic_rewards` /
   `vault_status` pattern (fetch during the build, serve statically, no
   runtime call to a non-allowlisted host). Render the Baro view from
   that. **Done when:** Baro renders data and the browser console shows
   zero CORS errors.
2. **Sentinels endpoint quirk.** `api.warframestat.us/sentinels/`
   returns a non-list shape and gets skipped, so Carrier Prime etc. are
   missing from `path_to_info`. Handle the real shape in
   `fetch_parent_data()`. **Done when:** sentinel prime parts resolve in
   the table.
3. **Sets card data.** Confirm it populates once warframestat returns
   200 (the preserve-on-empty logic now protects it across outages).
   **Done when:** `market.json.set_to_parts` is non-empty and the card
   shows recommendations against the project inventory.
4. **Verify sweep green:** vitest / pytest / cargo / svelte-check /
   vite build all pass.

## Tier 2 · Trust — safe to hand to others

5. **Security audit.** Run the `security-audit` workflow (or
   `/audit-security`). Resolve every open M1–M3 finding plus any new
   Critical/High. **Done when:** no Critical/High open and `SECURITY.md`
   matches what actually ships.
6. **Secret hygiene.** Re-confirm no `accountId` / `nonce` / JWT /
   passphrase reaches any log line, and that inventory data stays local
   (no exfiltration path). The `companion-security` agent owns this.

## Tier 3 · Distribution — a stranger can actually install

> Several steps here need a human (a chosen name, a real GitHub repo, a
> hosting account, a signed public release tag). The loop does all the
> surrounding **code, config, and local verification**, then **yields
> with a specific ask** — see "Human gates" below. Never invent a repo
> slug, product name, or hosting account.

7. **Name + repo.** Once the user supplies the name and repo slug,
   replace every `OWNER/REPO` placeholder: `prototype/public/install.sh`,
   `prototype/public/install.ps1`,
   `prototype/src/components/InstallWidget.svelte`. **Done when:** no
   `OWNER/REPO` remains anywhere and the widget's links resolve.
8. **Release pipeline.** Verify `release-companion.yml` emits Win+Linux
   binaries + `SHA256SUMS`, and that the installers verify against them.
   Dry-run the installers locally against a `WFMINV_BASE_URL` server
   before claiming this works. **Done when:** a clean-machine install
   succeeds end-to-end (or the local dry-run does, pending the human's
   release tag).
9. **Production hosting.** Move off GitHub Pages so `public/_headers`
   (CSP / HSTS / X-Frame-Options / `frame-ancestors`) actually applies.
   **Default target:** Cloudflare Pages. The loop prepares the config;
   the human connects the account. **Done when:** the security headers
   are live on the deployed site.
10. **Market refresh cron.** Confirm `refresh-market.yml` commits a
    **complete** `market.json` (the `csv_to_market_json.py` pipeline fix
    — never `wfm_demand.py --json-out`). **Done when:** a cron run (or
    `workflow_dispatch`) produces a snapshot with non-empty
    `set_to_parts` / `relic_rewards` / `vault_status`.

## Tier 4 · Onboarding & the differentiator — polished, not just usable

11. **First-run onboarding.** The empty state must walk a brand-new user
    with no external docs: install companion → `setcap` → fetch → drop
    `inventory.json` → results. Verify in a real browser with an empty
    `localStorage`. **Done when:** a no-docs user reaches a populated
    table.
12. **Validate the differentiator.** Sanity-check the sell-score against
    reality: pick 5–8 owned items, compare the score's ranking against
    their live WFM demand/price, and confirm the ranking is defensible.
    Surface the formula in-app (the explainer is partly there). **Done
    when:** the top-of-table recommendations are demonstrably sensible
    and documented.

---

## What each fire does

1. Read `CLAUDE.md`, `prototype/CLAUDE.md`, `companion/CLAUDE.md`,
   `scripts/CLAUDE.md`, the relevant component/source files, AND the
   loop journal at
   `/home/prowly/Desktop/Warframe market check/.loop-journal.md`.
   Identify what's already done by inspection — don't redo shipped work
   or re-walk journal dead-ends.
2. Pick the next unfinished item from the lowest open tier.
3. Ship the smallest end-to-end slice (data layer → UI → test).
4. Run the full verify sweep:
   - `cd prototype && bun run test`
   - `cd prototype && npx svelte-check`
   - `cd prototype && bun run build`
   - `/home/prowly/.local/bin/pytest /home/prowly/Desktop/Warframe\ market\ check/tests/ -q`
   - `cd companion && cargo test --release --quiet` if Rust changed
   Red → fix before claiming done.
5. For any **UI** change, drive Playwright with the project's
   `inventory.json`, look at the result, then stop the dev server and
   close the browser before yielding. Playwright is the source of truth
   for UI claims — "build succeeded" ≠ "feature works."
6. Update the relevant `CLAUDE.md` if the change introduced a new
   architectural rule (new `market.json` field, storage-key bump, new
   convention).
7. **Append a journal entry** (format below).
8. End the turn.

## Human gates — do the work, then yield with a specific ask

Some steps the loop cannot and must not do alone. When you reach one,
finish everything around it, write a journal entry, and yield with a
crisp request. **Do not fabricate.**

- **Product name + repo slug** (item 7) — the user picks these. Yield
  asking for the `owner/repo` and display name; until then, leave
  `OWNER/REPO` untouched.
- **Hosting account** (item 9) — prepare the Cloudflare Pages config and
  confirm `_headers`, then yield for the human to connect the account.
- **Public release tag** (item 8) — verify the workflow + local dry-run,
  then yield for the human to push the `v*` tag (an outward-facing,
  irreversible action).

## When stuck — argue before pushing through

If you hit a test failure you've tried once and it still fails, an
architectural fork with multiple defensible answers, an unexpected
WFM/warframestat response, or a UX decision the mockups don't settle —
you are STUCK. Summon an adversarial agent (`subagent_type:
general-purpose`): give it the decision, your reasoning, and "argue the
opposite — find the case I'm missing, with specific lines and failure
modes." Read its report; adjust if it has merit; iterate at most twice.
If still divergent, journal the unresolved dispute and yield. Record
each adversarial round in the journal.

## Defaults — don't ask, just pick

- **Baro data**: build-time bake into `market.json` (mirror
  `relic_rewards`), never a runtime fetch to warframestat.
- **Hosting**: Cloudflare Pages.
- **Onboarding copy**: lead with the one-time `setcap` (no recurring
  sudo); sudo is the "rather not?" fallback. Windows needs no elevation.
- **Sell-score validation**: if a WFM cross-check is ambiguous, document
  the assumption and move on — don't block v1 on a perfect metric.
- Honor every default already in `goal.md` for the four shipped features.

## Journal format

`.loop-journal.md` is append-only. Reuse `goal.md`'s exact format:
timestamp + fire N + item + `SHIPPED|YIELDED|BLOCKED`, then **Did /
Verified / Decisions / Next fire starts with / Open**. Read prior
entries before doing anything — they're the source of truth on what's
done and tried.

## Stop conditions (any one ends the loop)

1. **All four tiers complete** — the v1 end-state above is met and
   verified end-to-end in a real browser. Output:
   `V1 REACHED — <N> tests pass, all tiers shipped.` Do not re-fire.
2. A human gate is reached and no further tier-independent work remains
   — yield with the specific ask.
3. A test failure can't be resolved in a fire — yield with what failed
   and what's needed.

## Files of record

- Root / domain docs: `CLAUDE.md`, `prototype/CLAUDE.md`,
  `companion/CLAUDE.md`, `scripts/CLAUDE.md`, `SECURITY.md`
- App: `prototype/src/App.svelte`,
  `prototype/src/components/{ResultsTable,DropZone,InstallWidget,MyOrdersPanel,ListingReviewModal}.svelte`
- Backend: `scripts/csv_to_market_json.py`, `wfm_demand.py`,
  `companion/src/main.rs`
- CI: `.github/workflows/{refresh-market,release-companion,audit}.yml`
- Installers: `prototype/public/install.sh`, `prototype/public/install.ps1`
- Test inventory: `/home/prowly/Desktop/Warframe market check/inventory.json`
- Workflows: `security-audit`, `release-readiness`, `pre-commit-review`
