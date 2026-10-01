# Daily installation counts

Sharing is off until explicitly enabled in desktop Settings. A running app,
including one hidden in the tray, contributes at most one installation per UTC
day. Downloads and website visitors do not contribute. Multiple computers or
local app profiles can count separately. Resetting app storage can count again.
No total-user estimate, platform breakdown, retention funnel, or usage events
are produced. Synthetic submissions cannot be reliably prevented without
introducing identities; these are reported counts, not audited population data.

## Data flow and consent

Rust generates 32 bytes with the operating-system random generator. The only
POST field is `token`, formatted as `YYYY-MM-DD.<64 lowercase hex characters>`.
The date is part of the daily token, not an event timestamp. The client sends
the day's token at startup, on explicit opt-in, or at day rollover while running.
Until the service accepts it, the client offers the same token again once a
minute, at most ten times that UTC day, recording each attempt before it sends;
it stops at the first acceptance. Repeats carry the same token, so the service
counts the day once. There are no heartbeat requests and no queued old days: an
unaccepted day is abandoned at rollover. Once the day is accepted, the
once-a-minute local checks make no network requests. A day spent offline, or
whose ten attempts all fail, is absent from the count. A clock mismatch causes
rejection, not a contribution to another day.

The dedicated client sends no cookie, credentials, version, platform or WFM
headers and refuses redirects. Consent is enforced in Rust; private usage
settings cannot be read or changed through generic settings IPC commands.
Turning off cancels pending work; an already-delivered request cannot be undone.
If saving withdrawal fails, reporting stops for the session and the UI asks the
user to retry before restarting. Daily state survives restarts and same-day
opt-out/opt-in to avoid duplicate attempts. Corrupt daily state fails closed.

The collector atomically inserts a SHA-256 token hash and increments that day's
count only for a new hash. It stores no per-token metadata besides the UTC date.
Current-day hashes are purged on rollover and before traffic is accepted after
restart. SQLite uses secure deletion and DELETE journals; this is logical
retention, not a forensic-erasure claim about disks or snapshots. Exclude the
live database directory from host, volume, and VM backups.

The public endpoint returns up to 90 completed UTC days, with date, count,
coverage and update timestamp. Launch, restarts and detected maintenance gaps
(over two minutes) mark affected days incomplete. Zero means zero reports during
observed service coverage; it does not mean nobody used the app. A daily chart
cannot detect every network or upstream outage. Aggregates are retained indefinitely.

## Request quota

Two public routes exist - `POST /api/usage/check-in` and `GET /api/usage/daily` -
and they share one window of **20 requests per wall-clock second**, fixed in
`REQUESTS_PER_WINDOW` (`rust/tennoworth-usage/src/lib.rs`). It is one counter for
the whole service, not one per source: no client identifier is read or stored, so
there is nothing to key it by, and that is deliberate (see "Data flow and
consent"). The consequence worth knowing is that one sender can consume the window
for everyone. The ceiling bounds work; it is not a fairness mechanism and does not
protect count integrity.

It is a fixed one-second counter rather than a token bucket, so up to twice the
quota can pass across a window boundary, and it is not configurable at runtime.

Requests are charged **before** the body is parsed or the token is validated, so a
malformed or oversized request still consumes quota. That is the intended trade:
the counter exists to bound work before any parsing or database work happens, and
validating first would let a caller buy that work for free. The cost is that a
malformed request is indistinguishable from a valid one at the quota boundary,
which is acceptable - both are requests.

`/health` is deliberately outside the quota. It is loopback-only, and it is what
the monitor and the puller's post-restart check read; if it shared the public
window, someone else's flood would be reported as an outage and could roll back a
working binary.

## Recovery behaviour

The collector's `export` writes aggregates only, including today's partial
aggregate, and never token hashes; `usage.db`, its journals and state directory
are not backup material. `restore` preserves count maxima and marks the export
day through the recovery day incomplete, including days absent from the backup.
Lost deduplication state can produce repeated counts on that incomplete day.

A failed or stopped collector must not affect scans, trades, or app startup:
check-ins fail quietly. Monitoring GETs never contribute counts or record client
identifiers.

## Testing exclusions

Debug builds, unit tests, OCR test builds, and all probe modes never report to
production. Set `TENNOWORTH_DISABLE_USAGE=1` when launching an installed release
for maintainer testing on Windows or Linux. A normal release without that
exclusion still reports only after explicit consent.

The shared `tests/fixtures/usage/daily.json` pins the Rust/TypeScript public
contract. Collector HTTP tests use ephemeral loopback ports. For an isolated service run,
set `TENNOWORTH_USAGE_DB` and `TENNOWORTH_USAGE_PORT`; the listener always binds
loopback. Production defaults to port 8082. Test actual
settings and chart behavior in both browser engines/themes, and run desktop
probes with isolated application storage.
