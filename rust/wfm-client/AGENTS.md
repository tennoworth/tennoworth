# wfm-client/ - WFM transport primitives

Parent rules: [`../AGENTS.md`](../AGENTS.md). This crate owns how a request is
built and sent; what the request means is
[`../wfm-core/AGENTS.md`](../wfm-core/AGENTS.md)'s business.

Share primitives only. Do not grow this crate into an abstraction that swallows
authed order mutation - the trading services stay in `wfm-core`.

## Headers

Every `api.warframe.market` call carries `Crossplay: true` + `Platform: pc` +
`Language: en`, which is what `wfm_headers()` sends.

**Signin is the documented exception and is NOT a bug.** It POSTs to
`warframe.market/v1/auth/signin` with `Platform` + `Language` + `auth_type` +
`X-CSRFToken` and no `Crossplay`, which is what works against the live
endpoint. It is an auth-host request, not an API call, and it is verified
working as written. Don't "fix" the omission by adding the header.

Authed calls additionally set `Cookie` + `Origin` + `Referer` through
`wfm_authed_headers()`. Build headers through these helpers rather than
hand-rolling `.header(...)` chains at a new call site, so the set stays uniform
as endpoints are added.

## User agent

Always the descriptive project UA, built by
`wfm_client::user_agent(component, version)`. WFM's rules (ToS §11) REQUIRE it
and treat browser spoofing as block-worthy. The old Firefox `BROWSER_UA` is
gone (2026-08-16 - probed: a descriptive UA is accepted on every v1/v2
endpoint).

## Budgets, pacing and retry

- Every WFM HTTP attempt, including retries, goes through this crate's
  transport. Callers must not add independent sleeps or retry loops.
- The current request-budget, classification and reconciliation policy is in
  [`../../docs/wfm-access.md`](../../docs/wfm-access.md). Read it before
  changing limits or backoff.
- Network calls go through `wfm_client()` so the user-agent and timeout policy
  applies uniformly.
- The pacing constants are parity-gated by the shared
  `tests/fixtures/pacing.json` fixture. A constant that lives inside the part of
  a test that is stubbed out is unguarded no matter how thorough the suite
  around it looks - that is why the fixture, not a comment, is the gate.

## Retry and reconciliation boundary

Retry/backoff and envelope unwrapping live here. **Mutation reconciliation does
not**: it stays in the trading services, and an ambiguous create is never
blindly retried. Classify outcomes using the policy in
[`../../docs/wfm-access.md`](../../docs/wfm-access.md).
