---
description: Full security audit. Dispatches companion-security + browser-security agents in parallel, consolidates findings.
---

Spawn `companion-security` and `browser-security` agents in
parallel (single message, two Agent tool calls). Brief each with:

For `companion-security`:
> Full audit of the Rust companion (`companion/`). Walk
> `SECURITY.md`'s commitments and verify the code lives up to them.
> Cite file:line for every finding. Group by Critical / High /
> Medium / Low. Under 800 words.

For `browser-security`:
> Full audit of the browser app (`prototype/`). CSP, crypto,
> storage, XSS, install scripts. Cite file:line. Group by Critical /
> High / Medium / Low. Under 800 words.

After both return, merge the findings into a single report sorted by
severity (Critical first). Dedupe anything covered by both. Don't
write fixes yet — present the report and ask the user which to
tackle.

If `cargo audit` or `npm audit` show advisories, surface those at
the top.
