# Trade Session implementation

Work in progress; the desktop planner is not enabled yet.

The approved scope is a dedicated desktop view with Fast Cash, Plat per Trade,
Clear Inventory, and Max Value. There is no timer or duration prompt. Budget
defaults to the smaller of eight, the observed allowance, and the listing batch
cap. An optional platinum target uses credible listing asks, not executable bids.
Goals and the general recipe graph are separate work.

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
and safe available quantities still need enforcement in the planner/review path;
catalog bulk capability alone is not sufficient inventory or trade-slot evidence.

The current [WFM order contract](https://docs.warframe.market/docs/api/orders/)
permits `perTrade` on both create and update only for bulk-tradable items.
[Item metadata](https://docs.warframe.market/docs/data-models/#item) supplies the
capability. Product price caps remain unchanged.

### Pending-file compatibility

The optional field preserves reading of existing shipped pending files. Their
missing lot retains the previous inferred-create/unchanged-update behavior.
Explicit lots survive serialization and recovery without re-inference. Older
executables do not understand this field: do not resume a newly created explicit-
lot batch after downgrading; finish or discard it using the supporting version.

`tests/fixtures/trade-session/lots.json` records validation and create/update
wire expectations. Rust tests also cover catalog capability, absent fields,
disk recovery, and rejection before an existing order mutation. The eventual
frontend lot validator must consume the same fixture as its parity gate.

## Remaining implementation

- Rust-owned account-scoped allowance observations from scans; stable EE.log
  deduplication, scan/event reconciliation, confidence, restart and UTC reset.
- Deterministic four-mode selection using the shared sellable-quantity policy,
  verified item eligibility, credible prices, hold penalties and target handling.
- Dedicated view using the shared design system, with scan/unknown/zero states,
  bounded requested price refresh and per-row trade assumptions.
- Existing review integration, explicit before/after order differences,
  preservation of edits, and submission/recovery revalidation.
- Browser and native Windows/Linux workflow verification before shipping.

Creating a listing must never decrement the in-game allowance. No live WFM
mutation is part of implementation verification without separate authorization.
