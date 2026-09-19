# What the app does when warframe.market is unreachable (investigation, 2026-09)

Investigation only: no code change is proposed here. The question came from the
finding that a day-long warframe.market outage is a community-level event
(`docs/reddit-pain-points-2026-09.md`, F8), and from the observation that the
snapshot refresh path cannot answer it.

## The distinction that matters

**A failed snapshot refresh is not evidence that warframe.market is down.**
The snapshot is fetched from `tennoworth.app`, not from WFM
(`rust/tennoworth-desktop/src/services/market.rs`), and that path deliberately
treats offline, HTTP failure and "not modified" as the same quiet outcome by
keeping the cached copy. So "prices look stale" and "WFM is unreachable" are
different states, and only the second one affects login, live checks, listings
and watches.

Any future work here must distinguish five states, because they have different
user-facing consequences:

| State | What is actually unavailable |
|---|---|
| Snapshot host (`tennoworth.app`) unreachable | Nothing WFM-related; the bundled or cached snapshot stays in use |
| warframe.market API unreachable (DNS, timeout, connection refused) | Live top-of-book, listings, catalog fetch, riven comps |
| Authentication failure | Everything behind a session: listings, own orders, live checks that exclude own orders |
| 429 / 509 cooldown | All WFM access, for the duration the server stated |
| Policy pause (`docs/wfm-access.md`) | Whatever the signed policy restricts; may be background-only |

## What already exists

Verified by reading the tree; each of these is existing behaviour, not
something to rebuild:

- **Cached and bundled snapshot startup, with strictly-newer replacement** -
  `frontend/src/features/inventory/controller.svelte.ts` (a server rollback
  serving an older snapshot cannot replace a newer cached one).
- **A snapshot staleness banner** - `frontend/src/features/selling/SellPane.svelte`
  ("Prices may be outdated - this market snapshot is N old").
- **Cooldown and policy explanations with mutation gating** -
  `frontend/src/features/selling/controller.svelte.ts` (cooling-down copy with a
  deadline, per-restriction pause list, and a mutations-blocked flag that
  disables the write affordances).
- **Interrupted batches stay saved and require explicit Resume** -
  `rust/wfm-core/src/trading/plan.rs` and the Resume/Discard row in
  `frontend/src/shells/DesktopShell.svelte`.
- **Per-item live-check failures degrade inline** - `LiveTop.error` is set per
  queried item and the row shows "no live data" rather than losing the batch
  (`rust/wfm-core/src/trading/live_top.rs`).
- **Automatic listing closure on a detected sale is separately gated** -
  `rust/tennoworth-desktop/src/services/trades.rs`.

So the app is not fragile in this area; the open question is whether the *user
can tell which state they are in*, especially for a plain WFM API outage that
is not a cooldown or a policy pause.

## What was not done

The runtime half of this investigation was **not** performed in this change:
driving the installed desktop app with warframe.market unreachable and
recording each surface's actual state. That needs a native run on both
platforms and cannot be replaced by reading code.

The procedure to settle it:

1. Block `api.warframe.market` and `warframe.market` at the resolver (not
   `tennoworth.app`), then start the app with a scan already cached.
2. Record what each surface shows: Sell view, My orders, listing review, Trade
   Session, Rivens comps, price watches, the Ledger, and the notification inbox.
3. Induce a 429 (or use an existing cooldown) and repeat, to check that a
   plain outage is distinguishable from a cooldown in the copy.
4. Restore connectivity **without restarting**, and confirm each surface
   recovers on its own rather than staying disabled.
5. Restart the app while still blocked, and confirm a pending batch is still
   offered for Resume and is not silently completed or discarded.
6. Repeat on Windows and Linux.

## Conclusion

No implementation is proposed. The static reading shows the failure handling is
already deliberate in six places, and the remaining risk - an outage reading as
a generic error, or a surface that never recovers without a restart - can only
be confirmed by step 4 above. If that step shows a surface stuck in a disabled
state after connectivity returns, the fix belongs where the disable decision is
made (`frontend/src/features/selling/controller.svelte.ts` and the individual
feature controllers), not in a new app-wide overlay.
