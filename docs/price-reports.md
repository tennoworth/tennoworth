# Price reports

An opt-in service that collects anonymous sale prices from desktop installs and
publishes them as weekly aggregates. It also collects riven fingerprint
*shapes*: the names of keys the app cannot read yet, so a change in DE's
riven format reaches the parser without anyone pasting raw data.

The service is `rust/tennoworth-reports`. The desktop app sends sale reports
once enabled in Settings → This app → Price sharing; it does not send riven
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
the previous one. Debug builds, tests, OCR test builds and probes never send,
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

**A riven shape** lists fingerprint keys the app does not read, the JSON type
of each value, the key names nested inside it, and unrecognised stat tags
(only `/Lotus/...` game paths). It never contains a value, an item id or a
weapon.

[`tests/fixtures/reports/contract.json`](../tests/fixtures/reports/contract.json)
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
kept indefinitely; `tennoworth-reports export` prints them, and only them.

An item's prices for a week are published only when at least **5 different
contributor ids** reported it. Each contributor's median unit price is one
vote; the published `p25`, `median` and `p75` are the spread of those votes,
so sending more reports buys no extra weight. Items under the floor are
dropped when their week folds.

`GET /api/reports/prices` returns the last eight closed weeks and the two open
ones (`complete: false`). `GET /api/reports/riven-shapes` returns shape counts
by distinct contributor for the same weeks; shapes hold no personal values and
have no floor.

## Limits

- A batch holds 1 to 50 sales from one ISO week, dated from seven days before
  the server's UTC day to one day after it.
- One contributor id may hold 500 sales per week; past that the service answers
  429.
- The public routes share one window of 20 requests per second, separate from
  the usage counter's. Bodies over 16 KiB are refused.
- Published responses are recomputed at most every five minutes.

## What this does not protect against

New contributor ids cost nothing, so one person can pose as five and get an
item published, or move its median. The floor makes this deliberate work, not
an accident, but it is not proof. Until there is evidence of how much that
happens, reported prices are shown beside warframe.market data and are not
used in sell scores.

HTTPS terminates at Cloudflare and the request passes through Caddy, as it
does for the usage counter; see [SECURITY.md](../SECURITY.md) for what the
delivery infrastructure can see.

## Running it

`TENNOWORTH_REPORTS_DB` sets the database path (default
`/var/lib/tennoworth-reports/reports.db`) and `TENNOWORTH_REPORTS_PORT` the
loopback port (default 8083). `/health` is outside the quota and answers 204
while the maintenance tick, which runs every 30 seconds, is current.
