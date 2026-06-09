---
description: Verify warframe.market endpoints still return the shapes our code expects. Delegates to the wfm-api-shape-check agent.
---

Spawn the `wfm-api-shape-check` agent via the Agent tool. Pass it
this brief:

> Audit the WFM endpoints this project depends on. Sample 3–5 items
> from `/v2/items`, confirm the fields `companion/src/main.rs`
> `fetch_wfm_catalog()` reads still exist (`id`, `slug`,
> `i18n.en.name`). Then sample `prototype/public/market.json`,
> confirm its shape matches what `prototype/src/lib/market.js`
> parses. Report any drift, sorted by Breaking / Latent.
> Read-only only — no POST/PATCH/DELETE. Cap at 8 WFM requests total.
> Under 200 words.

After the agent returns, show the user its report verbatim and ask
whether to propose code changes for any Breaking findings.
