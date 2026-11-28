# Price reports

An opt-in service that collects anonymous sale prices from desktop installs and
publishes them as weekly aggregates. Riven sales are reported by weapon and
roll verdict, and players can rate the verdicts of the
[riven roll guide](../CONTRIBUTING.md#riven-roll-guide). It also collects riven
fingerprint *shapes*: the names of keys the app cannot read yet, so a change in DE's
riven format reaches the parser without anyone pasting raw data.

The service is `rust/tennoworth-collectors/src/reports`. The desktop app sends sale reports
once enabled in Settings → Privacy & sharing → Price sharing, or from the invitation
the app shows above its views (at most three times, two weeks apart, and never
once enabled); it does not send riven
shapes yet, and they will have a switch of their own. There are no TennoWorth
accounts.

## How the app sends sales

The trade ledger is the queue. Enabling price sharing records the newest
trade already in it, and only trades recorded after that are ever read, so
history from before consent is never sent. Each minute the app reads the
trades it has not handled yet, skips any that are not plat for one kind of
item or are older than seven days, and sends the rest in batches. A batch the
service accepts or refuses for good is settled; a network or server failure
keeps the trades for the next pass, with the pause doubling up to 30 minutes.
Turning sharing off stops the next request and cancels one in flight.

"Delete recent reports" in Settings sends the erase secrets for this week and
the previous one. It waits for an active upload pass to finish and persists a
queue boundary before contacting the service, so trades already recorded at
that boundary are not uploaded again, even if deletion must be retried. New
trades remain eligible while sharing is enabled. A failed deletion must be
retried to remove reports already held by the service.

Debug builds, tests, OCR test builds and probes never send,
and neither does a release launched with `TENNOWORTH_DISABLE_USAGE=1`.

## What a report contains

**A sale** is one trade the game confirmed in EE.log in which platinum was
exchanged for a single kind of item. Mixed trades and multi-item bundles are
not reported, because no unit price can be read from them. A batch is:

```json
{ "v": 1, "contributor": "<64 hex>", "erase": "<64 hex>",
  "sales": [{ "ref": "<64 hex>", "day": "2026-10-08", "side": "sale",
              "slug": "primed_flow", "tiered": true, "qty": 1, "plat": 40 }] }
```

- `slug` comes from the local catalog. A name the catalog cannot match is not
  sent, so no free text leaves the machine.
- `tiered` marks items with a rank or subtype. EE.log does not record ranks, so
  these prices are published separately and should read as "rank unknown".
- `ref` lets a resent batch count once.
- Never sent: the trading partner, the time of day, the game's log stamp, the
  item's display name, the app version, the platform, inventory, the game
  account id or any warframe.market detail.

**A riven sale** is a sale of one riven the app can identify. The trade
dialog names a riven by its weapon and a generated word ("Acceltra Critacan"),
never its stats, so the app matches the name against the rivens its last two
inventory scans held. From each scan it keeps only each riven's weapon and
roll verdict, and only while riven sales are covered. When that weapon's
rivens disagree on a verdict, or no scan held one, the sale is not reported.
Bought rivens are never reported. A batch is:

```json
{ "v": 1, "contributor": "<64 hex>", "erase": "<64 hex>",
  "sales": [{ "ref": "<64 hex>", "day": "2026-10-08", "weapon": "acceltra",
              "verdict": "sellable", "plat": 150 }] }
```

`verdict` is `sellable`, `almost`, `weak` or `unrated` (no guide entry, or a
stat the app cannot name). Never sent: the riven's stats, its name, its
rerolls or rank, and anything listed for sales above. A stat set and a price
together can match a public listing, which would tie a weekly id to a seller.

Installs that turned price sharing on before riven sales existed agreed to
terms that excluded riven trades. They send no riven sale until they choose
"Include riven sales" in Settings; turning sharing on now covers them.

**A rating** answers whether the verdict the app showed for one riven is
right. It is sent only when the player clicks, and only with price sharing on:

```json
{ "v": 1, "contributor": "<64 hex>", "erase": "<64 hex>", "weapon": "acceltra",
  "verdict": "weak", "agree": false,
  "suggest": { "stat": "zoom", "positive": false, "good": true } }
```

`suggest` is optional and only accompanies disagreement: one stat and its sign,
and whether it is good on this weapon (a wanted positive or a harmless
negative). The app offers only the opposite of what the guide said about the
riven's own stats. A later rating of the same weapon in the same week replaces
the earlier one.

**A riven shape** lists fingerprint keys the app does not read, the JSON type
of each value, the key names nested inside it, and unrecognised stat tags
(only `/Lotus/...` game paths). It never contains a value, an item id or a
weapon.

[`tests/fixtures/reports.json`](../tests/fixtures/reports.json)
holds accepted and refused examples of each.

## Contributor ids

The install keeps a random key that never leaves the machine. For each ISO week
it derives a `contributor` id and an erase secret from that key, and sends the
SHA-256 of the secret as `erase`. The service therefore sees an id that
changes every week and cannot link one week's reports to the next.

Within a week, one install's reports share an id, so the service can tell they
came from the same install. That is what lets each install count once.

To withdraw, the app sends the week and its erase secret to
`POST /api/reports/erase`, which deletes every report of that week carrying the
matching hash. It answers 204 whether or not anything matched. Only the two
open weeks can be erased; see below.

## Retention and publishing

Raw reports exist only for the current and the previous ISO week: a sale up to
seven days old can still arrive. When a week closes, its reports are folded
into aggregates and deleted, ids and erase hashes included. Aggregates are
kept indefinitely; `tennoworth-reports export` prints the price aggregates
and `tennoworth-reports export-rivens` the riven price and rating aggregates,
and nothing else.

An item's prices for a week are published only when at least **5 different
contributor ids** reported it. Riven prices are published the same way, per
weapon and verdict. Each contributor's median unit price is one
vote; the published `p25`, `median` and `p75` are the spread of those votes,
so sending more reports buys no extra weight. Items under the floor are
dropped when their week folds.

`GET /api/reports/prices` returns the last eight closed weeks and the two open
ones (`complete: false`). `GET /api/reports/riven-prices` does the same for
riven prices, and `GET /api/reports/roll-votes` for ratings. `GET /api/reports/riven-shapes` returns shape counts
by distinct contributor for the same weeks; shapes hold no personal values and
have no floor.

## Ratings and the roll guide

A rating never edits the guide. Each week, for each weapon at least
**5 contributor ids** rated, `roll-votes` publishes how many agreed with each
verdict and how many backed each correction. A correction is `proposed` when
at least 5 ids back it, at least **2** of them reported selling a riven for
that weapon the same week, and they are more than half of the weapon's
voters. A proposed correction is evidence for a maintainer, who changes
`roll-guide.json` through a reviewed pull request like any other edit.

New ids are free, so five fresh ids can rate anything. A reported riven sale
needs a trade the game confirmed, which is why a proposal also needs sellers.
That raises the cost of a flood without proving honesty; the review is the
safeguard.

## Limits

- A batch holds 1 to 50 sales from one ISO week, dated from seven days before
  the server's UTC day to one day after it.
- One contributor id may hold 500 sales per week; past that the service answers
  429. The same holds for 200 riven sales and ratings of 200 weapons.
- Reads (`GET`) and submissions each have their own window of 20 requests per
  second, separate from the usage counter's, so a flood of the public read
  routes cannot block submissions. Bodies over 16 KiB are refused.
- Published responses are recomputed at most every five minutes.

## What this does not protect against

New contributor ids cost nothing, so one person can pose as five and get an
item published, or move its median. The floor makes this deliberate work, not
an accident, but it is not proof. Until there is evidence of how much that
happens, reported prices are shown beside warframe.market data and are not
used in sell scores; a desktop test fails if client code starts reading
them. The Rivens view shows players' riven prices beside DE's band, labelled
as players', and `report_prices_unused.rs` lists that one reader as display
only.

HTTPS terminates at Cloudflare and the request passes through Caddy, as it
does for the usage counter; see [SECURITY.md](../SECURITY.md) for what the
delivery infrastructure can see.

## Running it

`build-reports` publishes the binary to the `reports-latest` rolling release
from `main`. On the host, `deploy/bin/pull-collector.sh reports` installs a checksum- and
signature-verified build (see SECURITY.md) and rolls back when `/health` does not answer after a restart, and
`deploy/bin/monitor-collector.sh reports` alerts when the published aggregates stop
updating. Caddy proxies `/api/reports/*` to the service with client headers
stripped and access logging off for those routes.

`TENNOWORTH_REPORTS_DB` sets the database path (default
`/var/lib/tennoworth-reports/reports.db`) and `TENNOWORTH_REPORTS_PORT` the
loopback port (default 8083). `/health` is outside the quota and answers 204
while the maintenance tick, which runs every 30 seconds, is current.
