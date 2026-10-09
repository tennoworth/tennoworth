# WFM access controls

Desktop features share one process-wide request governor. The scraper uses the
same implementation and a separate process budget. Clients contact WFM directly;
there is no account registry, central request proxy, or fleet-wide quota.

The compiled defaults are in `tests/fixtures/pacing.json`: 500 ms between HTTP
starts, two outstanding requests, 6.1 seconds between contract searches, and a
ten-minute watch interval. The waiting queue holds at most 128 requests. A waiting
background request gets a turn after four foreground requests. Cached quotes,
contract results and own orders last 15, 60 and 5 seconds respectively; final
listing validation always makes a fresh read.

Each request must declare its kind and priority before dispatch. Keep WFM sends
inside `wfm-client::transport`; authentication and reconciliation belong in the
existing core services. Non-WFM providers bypass this governor. New integrations
must extend `wfm-client/tests/send_sites.rs` and add request-budget tests. Redirects
involving WFM and the HTTP library's automatic retries are disabled so they cannot
create unaccounted attempts.

A `429` or `509` pauses the whole process. Retry-After dates and delta seconds are
never shortened. Missing or malformed headers use exponential cooldowns from
30 seconds to 15 minutes, plus positive jitter. Deadlines survive restarts, and
unrepresentable deadlines keep access paused. Short explicit `429` rejections can
be retried once for mutations; transport errors and server failures are ambiguous
and never trigger automatic mutation replay. Interrupted batches remain saved.
Resume requires explicit action and fresh reconciliation before every item. Stop after
the current request cancels unsent work while retaining the saved batch.

The bulk reader is patient only within limits, because abandoning a cycle costs
more traffic later. It waits out a cooldown when a single one is five seconds or
less, at most four times and twenty seconds in total per process; anything longer
stops the run for the next scheduled cycle, and a throttle whose deadline cannot
be read is never retried on a guess. Each throttle is recorded once - the wait
comes from the deadline the transport stored, so one response never grows the
backoff twice.

A read that still fails in transit or with a server error after the transport's
own attempts gets the same bounded treatment: the bulk reader retries it after
30 and then 90 seconds, through the transport, at most six waits and five
minutes per process. A client error is not retried. Without this, one dropped
connection among a sweep's thousands of reads abandoned the whole cycle.

## Host pipeline footprint

The scraper is the only client that reads WFM in bulk. These are the production
host's numbers on 2026-09-15, under the signed policy then in force - revision 1
of 2026-09-09, whose scraper restrictions are the compiled defaults above.

| Measure | Value | Basis |
|---|---|---|
| Hard ceiling | 2 request starts per second | governor spacing and in-flight cap |
| Requests per sweep | 6,438 | 1 catalog + 3,840 statistics + 2,597 orders, from the run's own summary |
| Sweep wall time | 60-89 minutes that day | `journalctl -u wfm-scrape.service` |
| Floor if nothing else were spent | 54 minutes | 6,438 x 500 ms |
| Cadence | every 2 hours plus up to 10 minutes of jitter | `deploy/wfm-scrape.timer` |
| Mean over a day | ~0.9 request starts per second | derived from the rows above |

The item loop runs one worker per request the policy allows in flight, rather
than one at a time: the same 176-request sample took 111 s serially and 91 s with
two workers, against an 88 s pacing floor. The request count, the cadence and the
ceiling are unchanged - the sweep stops paying for one response at a time, so its
wall time converges on the floor the pacing already implied. Overlapping is not
free: the summed fetch time of that sample - each attempt's wait for a governor
slot plus its request - rose from 110 s to about 190 s under two workers, which is
why the floor, not half the latency, is the bound. That counter is fetch time,
not upstream response time; the queue wait is inside it.

Each `scrape` and `build` run prints a `sweep metrics:` line with its attempt,
retry, throttle and byte counters, plus the cooldown and recovery waits it spent, including on
runs that fail - refresh this table from that line rather than from the
arithmetic. Its `elapsed_ms` is fetch time including the governor's pacing wait,
so it tracks wall time for a serial loop and exceeds it when workers overlap. The measured cost of the endpoints the
sweep chooses between is not uniform: one item's full order book was 96 KB,
while `/v2/orders/item/{slug}/top` answered the same item in 3.8 KB and the
`/v2/orders/recent` delta window in 216 KB.

## Signed-in status channel

warframe.market exposes the user's own status (Online, Online in game,
Invisible) only on its WebSocket: a client signs in on the socket with the
session's token and sends `@wfm|cmd/status/set`. The desktop keeps one such
socket open while a session is unlocked, beside the anonymous order stream. It
sends a message only when the status has to change and otherwise reads the
server's pushes, so its cost is the connection itself.

The server owns the status. Closing the socket does not clear one: a status set
with a duration stays public until the duration ends. Whatever the desktop keeps
up on its own - following the game, or a status kept while it runs - therefore
goes out with a 10-minute duration renewed every 5 minutes, and the app sends
Invisible when it quits or the session signs out. A change from another client
wins: the desktop stops renewing and pauses following until the next game
session. A sign-in the server refuses is not retried until the session changes.

## Policy signing key

Policies are signed with a dedicated Minisign key that the maintainer holds
offline, separate from the updater key; no private key enters GitHub. The public
key comes from the repository variable `TENNOWORTH_WFM_POLICY_PUBLIC_KEY`, which
the desktop, scraper and policy verifier embed at build time. Local development
without the variable uses compiled defaults and does not fetch remote policies.
Production release workflows require the variable and verify the deployed
policy before building clients.

## Create and publish a policy

The payload is JSON with `schema: 1`, an increasing positive integer `revision`,
`issued_at_ms` as a Unix timestamp in milliseconds, a plain-text `reason` of at
most 500 bytes, and complete `desktop` and `scraper` restriction objects. Use
`tests/fixtures/pacing.json` as the starting restriction object for both clients.
All fields are required. Spacing may increase up to one day; concurrency may
only decrease to one; the watch interval may increase up to seven days. The
pause flags independently restrict all access, background work, contracts,
mutations and WebSockets. Unknown fields and more permissive values are rejected.

The envelope is JSON holding the base64 payload and the complete Minisign
detached signature text, made in prehashed mode over the exact UTF-8 payload
bytes. The **publish-wfm-policy** workflow runs on `main` and verifies the
signature, schema, bounds and increasing revision against the previous
published envelope before updating the `wfm-policy-latest` rolling artifact.
Concurrent publications are serialized. On the host, a puller verifies a newer
envelope before atomically replacing the served file, and a policy URL that
answers 200 can still be the site's HTML fallback, so only a verified envelope
counts as published.

## Tightening and recovery

Publish a higher revision with the desired restrictions and a concise explanation.
Clients fetch at startup and every 15 minutes plus up to one minute of jitter, with
conditional requests, a five-second timeout and a 64 KiB response limit. Queued
requests recheck restrictions before dispatch. Requests already transmitted finish.
WFM WebSocket pauses close the existing stream and suppress reconnection. That
includes the signed-in status channel described below.

To restore access, publish another higher revision with restrictions removed, no
more permissive than compiled defaults. Never roll the revision backwards. Listing
batches do not resume automatically when a restriction or cooldown ends.

Network outages retain the last verified policy indefinitely. First launch without
a verified policy uses defaults. Invalid signatures and replays never replace a
verified envelope. A corrupt local safeguard file pauses access; cached inventory
and prices remain usable. Do not delete a cooldown file to work around a throttle.

Key replacement requires a desktop and scraper update. New clients embed the
replacement public key and need a policy signed with it. Retired public keys stay
listed in `wfm_client::policy::PREVIOUS_PUBLIC_KEYS`: they verify a policy that is
already cached on disk, so an updated client keeps that revision and its
restrictions, but a policy fetched from the network must verify with the current
key. The offline verifier applies the same rule to the previously published
envelope it compares against. Merely changing the build variable without listing
the retired key would correctly pause every client whose cache it cannot verify;
`a_rotated_key_keeps_the_cached_policy_but_not_its_authority` covers the migration.

A rotation runs in this order: generate and back up the new key; set
`TENNOWORTH_WFM_POLICY_PUBLIC_KEY` to it; sign and publish a policy with a higher
revision; redeploy the scraper with `scripts/deploy-scrape-host.sh`, which also
installs the matching verifier the box's policy puller uses; then release the
desktop. The redeploy runs while the box still serves the retired key's policy,
so its gate accepts that only when the published successor verifies with the new
key as a newer revision. Clients built before the rotation reject the new policy and keep their
last verified one. There is no remote key-replacement mechanism or expiry that
silently removes restrictions.

Counters for requests, cache hits/misses, throttles, queue rejections, queue depth
and outstanding work are local.
The status command and `wfm-access-changed` event expose no credentials or private
payloads. Updated clients cannot coordinate unrelated applications or computers
sharing an IP, and cannot enforce restrictions in older or modified clients.
