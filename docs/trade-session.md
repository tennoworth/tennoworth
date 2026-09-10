# Trade Session implementation

The desktop view prepares a batch for the existing listing review and executor.
This document describes its safeguards and remaining verification.

The approved scope is a dedicated desktop view with Fast Cash, Plat per Trade,
Clear Inventory, and Max Value. There is no timer or duration prompt. Budget
defaults to the smaller of eight, the observed allowance, and the listing batch
cap. An optional platinum target uses credible listing asks, not executable bids.
Protected copies, one pinned set goal, complete owned sets, and quantity-aware
buyer comparisons share this workflow. Purchasing missing components and a
general foundry planner remain outside its scope.

## Scan-only guidance

The Opportunities view, Set picks, and Baro can use locally protected inventory
estimates without a WFM session. Estimates subtract leveled, kept, reserved, and
pinned-goal copies, but do not subtract unknown WFM listings. They are labeled
as estimates and never enter listing review or Trade Session execution.
Connecting WFM and refreshing allocation replaces estimates with checked
availability only when the displayed inventory matches the latest native game
scan. Imported backups use their own quantities for protected estimates and
cannot inherit a native scan identity. Missing protection quantities remain
unavailable; unaffected estimates remain visible with partial totals labeled.

Every inventory-derived sell batch retains its reviewed native snapshot identity
in pending recovery. Native validation rejects a changed scan, imported snapshot,
or missing identity before each mutation. Older pending batches remain readable
but need a fresh scan and a newly prepared batch before posting. The local UI
cache is versioned separately from encrypted backups; older UI caches require
a rescan, while backups remain available for advisory restore.

## Listing foundation

`PlanItem.per_trade` carries an explicit reviewed lot through the existing plan
executor and pending recovery. Lots must divide the proposed total quantity and
be between one and six. A non-bulk item permits only a one-unit assumption and
omits `perTrade` from the WFM request. The catalog must explicitly declare
`bulkTradable: true` to send that field; do not infer capability from stack size.

Create uses the explicit lot, or the existing divisor default for callers that
omit it. Update carries an explicitly requested lot for bulk items, replaces
quantity rather than adding to it, and leaves visibility untouched. Existing
callers that omit a lot retain the prior PATCH behavior. Item-class eligibility
and safe quantities are checked in the planner, review, and native submission/
recovery path. Unknown classes, sets without verified recipes, refinements,
Rivens, and charge/star variants are excluded. The initial planner uses singles until an unlocked
catalog verifies supported bulk identities.

The current [WFM order contract](https://docs.warframe.market/docs/api/orders/)
permits `perTrade` on both create and update only for bulk-tradable items.
[Item metadata](https://docs.warframe.market/docs/data-models/#item) supplies the
capability. Product price caps remain unchanged.

WFM's `platinum` field is the total **lot** price. The planner and review use
unit prices; submission multiplies by the reviewed lot and rejects totals above
the price cap. Existing-order comparisons retain the raw lot total. Live quotes,
My Orders advice, and newly scraped snapshots divide by `perTrade`, preserving
fractional unit prices. Whole-platinum session asks round up, never below the
reference ask. The shared `prices.json` fixture checks Rust/frontend conversion.

New CSVs carry `price_basis=unit`, propagated to the snapshot only when present.
Legacy bulk book prices are not trusted as unit quotes: session planning uses
historical prices until a normalized snapshot or explicit live check is available.
Rebuilding an old CSV does not certify its prices or require a production scrape.

### Pending-file compatibility

The optional field preserves reading of existing shipped pending files. Their
missing lot retains the previous inferred-create/unchanged-update behavior.
Explicit lots survive serialization and recovery without re-inference. Older
executables do not understand this field: do not resume a newly created explicit-
lot batch after downgrading; finish or discard it using the supporting version.

`tests/fixtures/trade-session/lots.json` records validation and create/update
wire expectations, consumed by Rust and frontend validation tests. Pending items
also retain snapshot/day/budget constraints and exact existing-order expectations.
Failed recovery revalidation leaves unsubmitted items pending rather than silently
posting them.

## Allowance and inventory accounting

Normal and tray scans persist optional allowance observations. Account identity
is domain-separated and hashed locally; session credentials and raw log text are
not retained in the observation. Zero is known; missing/invalid metadata remains
unknown or a labeled mastery estimate.

Log identity uses a fixed-prefix digest containing the game's UTC startup header;
byte positions provide persistent replay protection even when a log is copied to
a new file. Unrecognized startup headers still permit
ledger/overlay processing, but cannot certify allowance tracking. Ledger insert,
deduplication, and allowance progress commit together. Duplicate callbacks never
repeat notifications or automatic order adjustments.

Scans capture before/after log boundaries. Earlier events are ignored; overlapping
trades retain the scan count and downgrade confidence. Callbacks committed between
cursor capture and observation persistence are reconciled under the database lock.
Monitoring gaps and log changes require a scan to restore tracking. Restart keeps
the observation's age but requires a fresh scan before submission. UTC reset uses
mastery only, never an inferred account bonus; old-day events cannot spend the new
day's allowance.

The shared sellable-quantity policy combines global/per-item reserves with
untradeable copies. An item given away since the scan is unavailable to the planner
until rescanned: EE.log cannot identify its rank. Submission/recovery checks the
snapshot, account observation, UTC day, safe quantities, supported identities,
reviewed orders, and total estimated trades before mutations.

## Selection and review

### Protected quantities and complete sets

The Protected selling plan disclosure is available in Sell, Trade Session,
Set picks and Baro. Manual quantities reserve unranked copies; a pinned set adds
its recipe quantities. The global keep-copy floor and existing per-item reserves
still apply. Leveled copies cannot satisfy a goal for unbuilt components.
Protection persists locally through `save_protection_plan`. Saving is excluded
while a listing plan runs. Missing recipes or unreadable protection do not
silently release copies.

`protection_state` reads native inventory and current WFM sell orders. Set orders
consume their component quantities, including repeated parts. The frontend uses
the resulting available counts across all four views. Without current orders,
listed and available quantities remain unknown, and Connect WFM offers the
authentication path. The allocation display distinguishes owned, protected,
listed and available copies. Existing orders are never changed by saving a goal.

Complete sets compete with individual parts for the same component pool. A set
must have a valid recipe fitting six game slots and a supported, rankless WFM
identity. It consumes one estimated exchange per set, with no purchase of missing
parts. Selection and edited review quantities cannot allocate the same copy
twice. Native submission and recovery also validate projected order totals;
updating an existing listing replaces its quantity rather than adding to it.
Quantity/rank edits in My orders revalidate active protection too.

Confirmed trades invalidate the affected scanned quantities until a new scan.
A reported set sale also invalidates its recipe components. Tray and notification
ranking read the stored protection requirements, so a goal cannot be bypassed
by switching recommendation surfaces. These controls do not establish the
accuracy of a game scan or guarantee that external clients leave orders unchanged.

### Buyer alternatives

Compare buyers opens a comparison for the selected item and quantity without
editing the listing batch. Rust retains buyer identity, status, platform,
quantity and lot price from the [WFM top-order response](https://docs.warframe.market/docs/api/orders/).
Only compatible visible buy orders count; own orders, unsupported variants,
invalid quantities and duplicate buyer identities are excluded. An unlocked
account is needed to establish the own-order exclusion.

The response covers at most five buyers. The comparison fills whole lots in
descending unit-price order, labels uncovered units, and compares the same
covered quantity against the selected listing reference. This heuristic does
not claim optimal book coverage. Missing books remain unavailable, empty books
show zero observed coverage, and observations expire after 60 seconds. Refresh
rechecks the selected identity. Item/profile links open WFM for deliberate
follow-up; the app does not send messages or promise a sale time.

The proposed player-pilot targets in the feature plan have not been measured.

### Batch selection

`tests/fixtures/trade-session/modes.json` pins deterministic mode behavior. Fast
Cash uses liquid singles. Plat per Trade offers each valuable candidate a first
lot before repeating. Clear Inventory prioritizes safe stack completion and
liquidity. Max Value may concentrate in a valuable position. These are explainable
heuristics, not mathematical optima. Hold advice lowers priority without bypassing
hard protection. Targets stop at listing-price coverage, allow a final-lot
overshoot, and report shortfalls.

The view does not inherit incidental Sell filters. Cached prices render first;
requested live checks are bounded to the selected batch with source labels. Unit
bids are comparisons, not executable stack proceeds. Review preserves edits during
refresh and resizing, recomputes trade arithmetic, and shows before/after quantity,
lot, price, and visibility. Changed/ambiguous orders require another review. WFM
does not provide an atomic compare-and-patch contract here: revalidation narrows
the race window but cannot prevent another client changing an order after the
final read.

## Native calculation data boundary

The desktop calculates protected sale scores and shared set/part allocation through
native domain IPC. Explicit availability caps apply before scoring; a missing entry
in a supplied availability map means zero available copies. Spares also retains the
global keep floor. Baro receives already-reserved quantities and does not subtract
another copy. Session results carry the component limits used by listing review;
the native listing path independently rechecks current protection before sending.


Trade selection and calendar advice accept numeric snapshot prices, with missing
or null prices remaining unknown. Numeric strings and other nonnumeric price
fields reject the calculation explicitly. This is stricter than the old browser
coercion; an invalid snapshot must not quietly produce a different suggestion.
History medians similarly require numbers or null. Unrelated history volume
metadata does not affect advice.

Finite inputs can still overflow when divided by a tiny positive baseline.
Native history and advisor validation reject those calculations before a nonfinite
result can serialize as null or appear as a recommendation. A literal zero
history baseline retains the existing `Infinity%` explanation; changing that
heuristic is separate from the port. Shared numeric-boundary fixtures preserve
legacy output and identify intentional native errors.

## Verification before shipping

Fixture/unit tests, styled Chromium/WebKit flows in both themes at narrow/short/
wide sizes, native probes, and the repository gates are required. Physical account
scans, actual trading across UTC reset, and Windows interactive display/scaling
remain manual acceptance work; browser/probe evidence does not certify gameplay.

Creating a listing must never decrement the in-game allowance. No live WFM
mutation is part of implementation verification without separate authorization.
