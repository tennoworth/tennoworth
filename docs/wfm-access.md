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

## Dedicated signing key

Generate a dedicated, passphrase-encrypted Minisign key on the maintainer's local
workstation. Keep it outside the checkout and the web server, in a private
directory. Do not reuse the updater key or use Minisign's `-W` option, which
disables encryption. Enter the passphrase in the terminal prompt, never as a
command argument, environment variable, or workflow secret.

```sh
umask 077
mkdir -p "$HOME/.local/share/tennoworth-signing"
chmod 700 "$HOME/.local/share/tennoworth-signing"
minisign -G -s "$HOME/.local/share/tennoworth-signing/wfm-policy.key" \
  -p "$HOME/.local/share/tennoworth-signing/wfm-policy.pub"
```

Back up the encrypted key and public key to offline storage, and retain the
passphrase separately in a password manager. Test backup recovery by signing a
disposable local file and verifying it with the public key. Losing the key or
passphrase prevents policy updates for existing clients. Encryption protects the
stored key; it does not protect signing on a compromised workstation.

Set the repository variable `TENNOWORTH_WFM_POLICY_PUBLIC_KEY` to the base64 public
key line from that public file. The desktop, scraper and policy verifier embed it
at build time. Local development without the variable uses compiled defaults and
does not fetch remote policies. Production release workflows require the variable
and verify the deployed policy before building clients.

## Create and publish a policy

The payload is JSON with `schema: 1`, an increasing positive integer `revision`,
`issued_at_ms` as a Unix timestamp in milliseconds, a plain-text `reason` of at
most 500 bytes, and complete `desktop` and `scraper` restriction objects. Use
`tests/fixtures/pacing.json` as the starting restriction object for both clients.
All fields are required. Spacing may increase up to one day; concurrency may
only decrease to one; the watch interval may increase up to seven days. The
pause flags independently restrict all access, background work, contracts,
mutations and WebSockets. Unknown fields and more permissive values are rejected.

Sign the exact UTF-8 payload bytes using modern Minisign's prehashed mode. Build
one envelope containing the base64 payload and the complete detached signature
text. For example, with jq installed on the administrative machine:

```sh
minisign -Sm payload.json -s "$HOME/.local/share/tennoworth-signing/wfm-policy.key"
jq -n --arg payload "$(base64 -w0 payload.json)" \
  --rawfile signature payload.json.minisig \
  '{payload: $payload, signature: $signature}' > wfm-policy.json
base64 -w0 wfm-policy.json
```

Run **publish-wfm-policy** on `main`, providing that final base64 string. The workflow verifies the signature,
schema, bounds and increasing revision against the previous published envelope
before updating the `wfm-policy-latest` rolling artifact. No private key enters
GitHub. Concurrent publications are serialized.

For initial setup only, run the workflow on the tested `develop` revision with
`first_publication: true`. This builds the verifier on Ubuntu 22.04 before the
first policy-dependent production promotion. The bootstrap rejects an existing
policy release, any other branch, or an unknown release-lookup response; all
later revisions must be published from `main`. Configure the public-key variable
before running it and use revision 1 with the compiled defaults and no pauses.

For the first deployment, install the `wfm-policy` verifier artifact from that
release at `/srv/wfm/bin/wfm-policy`, owned and installed like the existing scraper
binary. Install `pull-policy.sh`, its service/timer, and the Caddy policy route.
The puller verifies a newer envelope before atomically replacing the served file.
Enable `wfm-policy-pull.timer` after installing the verifier. Publish and verify a
policy matching compiled defaults before releasing desktop or scraper clients.
Fetch the public policy URL and verify it with the built verifier before
promotion: an HTTP 200 can still be the site's HTML fallback. Preserve the live
market files while installing the route and policy services.

## Tightening and recovery

Publish a higher revision with the desired restrictions and a concise explanation.
Clients fetch at startup and every 15 minutes plus up to one minute of jitter, with
conditional requests, a five-second timeout and a 64 KiB response limit. Queued
requests recheck restrictions before dispatch. Requests already transmitted finish.
WFM WebSocket pauses close the existing stream and suppress reconnection.

To restore access, publish another higher revision with restrictions removed, no
more permissive than compiled defaults. Never roll the revision backwards. Listing
batches do not resume automatically when a restriction or cooldown ends.

Network outages retain the last verified policy indefinitely. First launch without
a verified policy uses defaults. Invalid signatures and replays never replace a
verified envelope. A corrupt local safeguard file pauses access; cached inventory
and prices remain usable. Do not delete a cooldown file to work around a throttle.

Key replacement requires a desktop and scraper update. Keep the old key available
long enough to publish recovery instructions for old clients; new clients embed
the replacement public key and need a policy signed with it. The rotation release
must also migrate the previous signed cache: verify it using the previous public
key for local migration only, preserve its revision/restrictions, and accept new
network policies only with the replacement key. Merely changing the build variable
will correctly pause clients whose existing cache cannot be verified. Test that
migration before a key change. There is no remote
key-replacement mechanism or expiry that silently removes restrictions.

Counters for requests, cache hits/misses, throttles, queue rejections, queue depth
and outstanding work are local.
The status command and `wfm-access-changed` event expose no credentials or private
payloads. Updated clients cannot coordinate unrelated applications or computers
sharing an IP, and cannot enforce restrictions in older or modified clients.
