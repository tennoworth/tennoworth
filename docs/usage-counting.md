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
The date is part of the daily token, not an event timestamp. The client records
its attempt before sending: at most one attempt per UTC day, at startup,
explicit opt-in, or day rollover while running. There are no heartbeat requests,
retries, or queued old days. Local timer checks make no network requests.
Offline time and failed attempts are absent from the count. A clock mismatch
causes rejection, not a contribution to another day.

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

## Server installation

Production deployment requires the normal explicit production authorization.
Artifacts come from `build-usage.yml` on `main`, with a checksum. The collector
runs on `127.0.0.1:8082`; confirm that port is free. The route uses `skip_log`, supported by the distribution Caddy and newer releases. On the existing box, as root:

```sh
useradd --system --no-create-home --shell /usr/sbin/nologin tennoworth-usage
install -m 0755 /srv/wfm/app/deploy/pull-usage.sh /srv/wfm/pull-usage.sh
install -m 0755 /srv/wfm/app/deploy/monitor-usage.sh /srv/wfm/monitor-usage.sh
install -m 0644 /srv/wfm/app/deploy/tennoworth-usage*.service /etc/systemd/system/
install -m 0644 /srv/wfm/app/deploy/tennoworth-usage*.timer /etc/systemd/system/
systemctl daemon-reload
/srv/wfm/pull-usage.sh
systemctl enable tennoworth-usage.service
systemctl enable --now tennoworth-usage-pull.timer tennoworth-usage-monitor.timer
```

Skip user creation if the dedicated account exists. Merge the usage route from
the repository Caddyfile into the live site block, validate, then reload.
Preserve unrelated sites. Install changed service files and reload systemd when
updating configuration; binary pulls alone do not install configuration.

Before opening the public POST, review Cloudflare Logpush/Logpull, security-event
retention, cloudflared logging, Caddy access/error logging, and host snapshots.
Exclude these routes from controllable request logs. Include the repository’s
usage error handler: access-log exclusion alone does not suppress Caddy’s
default proxy-failure logs, which contain request metadata. Keep runtime debug
logging disabled and verify the stopped-collector case as well as successful
requests. Never enable request-body
or debug tracing. Caddy strips identifying headers before forwarding, but
Cloudflare terminates TLS and can retain its own operational/security metadata.
Do not advertise zero provider retention. Record actual reviewed settings with
deployment evidence.

Verify loopback `/health` returns 204 and public `/api/usage/daily` returns JSON.
Exercise synthetic requests against a separate test database, never the
production counter. Confirm retained logs contain no bodies, tokens or IPs and
website GETs do not count. Release desktop opt-in after this verification.
Existing users remain opted out through upgrades.

## Backup, recovery, rollback and monitoring

Export aggregates only, as the service account. Stage output and rename only on
success. The export includes today's partial aggregate, never token hashes:

```sh
sudo -u tennoworth-usage /srv/wfm/bin/tennoworth-usage export > usage-counts.json.new && mv usage-counts.json.new usage-counts.json
```

Schedule this in the existing backup system; never back up `usage.db`, journals,
or its state directory. For recovery, stop the collector and feed the aggregate
export to `tennoworth-usage restore` as the service account, then restart.
Restore preserves count maxima and marks the export day through recovery day
incomplete, including days absent from the backup. Loss of
deduplication state can produce repeated counts on that incomplete day.

The puller keeps the preceding binary and restores it on failed health checks.
To stop collection immediately, stop the collector; check-ins fail quietly.
A failed endpoint must not affect scans, trades, or app startup.

The systemd service alerts through the existing failure unit. The usage monitor
timer checks loopback health and aggregate freshness every five minutes and
alerts through the same unit if maintenance is over two hours old. Monitoring
GETs never contribute counts or record client identifiers.

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
