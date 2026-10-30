# wfm-client/ - WFM transport primitives

Parent rules: [`../AGENTS.md`](../AGENTS.md). This crate owns how a request is
built and sent; what the request means is
[`../wfm-core/AGENTS.md`](../wfm-core/AGENTS.md)'s business.

Share primitives only. Do not grow this crate into an abstraction that swallows
authed order mutation - the trading services stay in `wfm-core`.

## Headers

Every `api.warframe.market` call carries `Crossplay: true` + `Platform: pc` +
`Language: en`, which is what `wfm_headers()` sends.

Sign-in makes no request of its own: every `warframe.market`
page is behind a Cloudflare bot challenge, so the user signs in on WFM's page
in a desktop webview and the app keeps the resulting `JWT` cookie. Do not
reintroduce a scripted CSRF + password POST; it cannot pass the challenge.

Authed calls additionally set `Cookie` + `Origin` + `Referer` through
`wfm_authed_headers()`. Build headers through these helpers rather than
hand-rolling `.header(...)` chains at a new call site, so the set stays uniform
as endpoints are added.

## User agent

Always the descriptive project UA, built by
`wfm_client::user_agent(component, version)`. WFM's rules (ToS §11) REQUIRE it
and treat browser spoofing as block-worthy. A descriptive UA is accepted on
every v1/v2 endpoint.

## Budgets, retry and reconciliation

The request-budget, classification, retry and reconciliation policy is
[`../../docs/wfm-access.md`](../../docs/wfm-access.md); read it before changing
limits or backoff.

- Every WFM HTTP attempt goes through this crate's transport, which owns the
  per-request retries (reads three attempts, a short `429` once for a mutation).
  The only retry layered above it is the bulk reader's sweep recovery in
  `wfm-scrape/src/http.rs` - long, budgeted waits that belong to an unattended
  sweep and must never reach an interactive caller - and it sends every attempt
  back through the transport. Do not add a third.
- **Mutation reconciliation is not transport**: it stays in `wfm-core`'s
  trading services, and an ambiguous create is never replayed.
- Clients carry the descriptive `user_agent()`; in `wfm-core` they are built by
  `wfm_core::http::wfm_client()`, so the user-agent and timeout policy applies
  uniformly there.
- The pacing constants are gated by `tests/fixtures/pacing.json` on both sides,
  because the sleeper is stubbed in every test that exercises them.
