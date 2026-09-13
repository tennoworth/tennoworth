# wfm-scrape/ - DE ingest and the host market pipeline

Parent rules: [`../AGENTS.md`](../AGENTS.md). This file covers
`wfm-scrape/src/de.rs` and `de_extract.rs` - the Digital Extremes ingest - and
the observation-state contracts the pipeline enforces.

## Digital Extremes ingest

DE publishes no documented API, no keys and no rate limit. These are the
endpoints the game and the official Companion app run on; a decade of community
use has been tolerated, not licensed. Fetch them from the host pipeline once per
cycle, with the descriptive UA, and never from a visitor's browser.

- **Every DE URL lives in `de.rs`.** Five of the paths the community wiki
  documents are already dead - including the worldState URL most guides still
  quote. When the next one moves, one file changes.
- **The index is LZMA-*alone*, not xz.** `lzma_rs::lzma_decompress`, not
  `xz_decompress`. And the content hash is **part of the manifest path** -
  the bare filename 404s.
- **Manifests are not single-key.** `ExportWeapons_en.json` carries both
  `ExportWeapons` (837) and `ExportRailjackWeapons` (143). Use
  `manifest_rows_for(doc, basename)`; "the first array" silently reads the
  railjack rows (it did, until the live test caught it).
- **`primeSellingPrice` keys on the RECIPE**, not its `resultType` - Nova
  Prime Blueprint is 45 ducats; the frame it builds has no ducat value.
- **Relic refinement odds are ours, not DE's.** The export ships four variants
  per relic (Bronze/Silver/Gold/Platinum) whose reward lists are identical;
  refinement changes the odds and DE does not publish them. The table lives in
  `de_extract::REFINEMENT_CHANCE` - re-check it on major updates.
- **Never resolve a `/Lotus/...` path by guessing.** `resolve_path` walks the
  `/Lotus/StoreItems` alias and then `ExportRecipes.resultType`, and returns
  `None` otherwise. ~87% of relic reward refs resolve; the rest (Forma, Kuva,
  Exilus adapters) are genuinely untradeable and *should* stay unresolved. A
  wrong price is worse than no price.
- **The live contract test is opt-in**, and is the thing that tells us when an
  endpoint moves:
  `cargo test -p wfm-scrape -- --ignored de_endpoints_are_still_alive`

## Observation-state contracts

- **Observation state comes from evidence, not output size.** DE surfaces use
  distinct unavailable, unchanged, invalid, usable, and authoritative-empty
  outcomes. Only a literal, schema-valid empty container may clear prior data;
  a missing key, malformed sibling, truncated join, or failed request carries
  the prior child and its prior data timestamp. Stamp independently fetched
  world-state/riven children independently - one fresh sibling must not make a
  carried sibling look fresh.
- **A shared child timestamp forbids partial truth.** Event Goals are either a
  wholly valid child or invalid/preserved; merging valid rows beside malformed
  siblings and then stamping the map fresh makes carried rows lie about age.
  Reward containers preserve item rewards and credits separately, distinguish
  unsupported-but-dated rewards as `unknown`, accept an explicit supported
  zero (`credits: 0`, empty supported arrays), and reject an unrecognized `{}`.
- **Annual usage has two contracts.** `usage_history` stores compact immutable
  year maps; current `usage` stores the newest rich MR curve used by scoring.
  Validate them separately. A valid compact latest year must still be
  selectively refetched when rich usage is missing, older, has an invalid
  `peak_mr`, or has an empty/non-finite `by_mr`; preserve compact history during
  repair and prove the following warm run skips the fetch. Missing years remain
  absent and retry - never zero-fill them or infer publication from the current
  calendar year.

The snapshot *shape* that desktop cache loading depends on is a cross-crate
contract, so it lives in [`../AGENTS.md`](../AGENTS.md) rather than here.

## Pipeline output

`scrape` runs the full WFM scrape to CSV; `build` renders `market.json` and
`wfstat-catalog.json` and is the complete snapshot/catalog generator. The
fixture regression gates in `tests/` shell the freshly built binary
(`env!("CARGO_BIN_EXE_wfm-scrape")`) against the frozen fixtures in
`tests/fixtures/{scrape,convert}` - cargo rebuilds the binary first, so a stale
one cannot green them.
