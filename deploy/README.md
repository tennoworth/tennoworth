# Self-hosting on home Proxmox VE

This app is a **static** Svelte build (`vite build` → `prototype/dist/`) plus a
**no-secrets Python cron scraper**. No backend, no database, no server-side
secrets, no auth. The companion (Rust CLI) runs on your **gaming PC**, not here.
So the server is just a static-file box + a cron that reads public APIs — its
attack surface is tiny. The real job is *being safe to expose from a home network*.

## Safe default (the whole thing in six moves)

1. **Unprivileged LXC** on Proxmox (Debian 13, 1 vCPU / 1 GB / 8 GB). Not a VM —
   the workload is static files + a Python script. Unprivileged means a container
   escape lands as a powerless host UID, not root.
2. **Isolated VLAN, default-deny**, with **no route to the Proxmox mgmt plane
   (8006/SSH) or the rest of your LAN**. Outbound only: 443 + cloudflared.
3. **Caddy** serves `dist/` and applies the real security headers
   ([`Caddyfile`](Caddyfile)) — the CSP/HSTS/frame-ancestors GitHub Pages dropped.
4. **Cloudflare Tunnel** for ingress → **zero inbound ports** on your router.
   Public URL, edge TLS/DDoS, home IP never exposed, works behind CGNAT.
5. **systemd timer** ([`wfm-scrape.timer`](wfm-scrape.timer)) runs the scrape
   every 2h; atomic `os.replace` means the browser never sees a half-written file.
6. **Build in CI, deploy `dist/`** — never run node/bun on the exposed box.

Most of steps 3–6 and the in-container install are automated by
[`setup-container.sh`](setup-container.sh).

---

## Already run Caddy + a Cloudflare Tunnel? (start here)

Then skip the install bits — you only need to *integrate*:

1. **Caddy:** paste the [`Caddyfile`](Caddyfile) block into your existing config
   (or `import` it). Pick a **free localhost port** — 8080 often collides, so the
   block uses **8081**; change it to whatever's free. Don't replace your Caddyfile.
2. **Tunnel:** add a **new public hostname** to your *existing* tunnel
   (`wfm.yourdomain.com → http://localhost:8081`). No new tunnel needed.
3. **Repo + scraper:** put the repo at `/srv/wfm/app`, create the venv +
   `wfm` user + the scrape timer (the relevant half of
   [`setup-container.sh`](setup-container.sh)), and deploy a CI-built `dist/`.

**One honest caveat about isolation:** if you run this *alongside* your other
services (shared Caddy/tunnel/container/host) rather than in a dedicated DMZ LXC,
you **don't get the network isolation** described below — this app then shares a
blast radius with everything else that box serves. This app's own surface is
tiny (static files + a no-secrets cron), so it adds little risk to the shared
box; the concern runs the other way — a vuln in a *neighbor* service can reach
this one. If any neighbor is risky, give this its own unprivileged LXC per below.

## Host steps (on the Proxmox node)

Edit the network bits (`tag=`, IPs, gateway) for your setup.

```bash
pveam update
pveam download local debian-13-standard_*_amd64.tar.zst

# Unprivileged container on VLAN 40 (a "DMZ" segment), per-NIC firewall on.
pct create 110 local:vztmpl/debian-13-standard_*_amd64.tar.zst \
  --hostname wfm-web --unprivileged 1 --features keyctl=1 \
  --cores 1 --memory 1024 --swap 512 --rootfs local-lvm:8 \
  --net0 name=eth0,bridge=vmbr1,tag=40,firewall=1,ip=10.40.0.10/24,gw=10.40.0.1 \
  --onboot 1 --ostype debian
pct start 110
```

**Network isolation (the part that makes a container compromise a non-event).**
On your router / L3 firewall, the DMZ VLAN must be **denied** to the Proxmox
mgmt IP and the LAN, permitted only to the internet:

```
deny   ip from 10.40.0.0/24 to <proxmox-mgmt-ip>   # no 8006 / SSH
deny   ip from 10.40.0.0/24 to <LAN-subnet>        # no lateral movement
permit ip from 10.40.0.0/24 to any                 # internet egress
```

On the **container's** Proxmox firewall tab: inbound policy DROP (cloudflared
dials out, so there's no inbound listener to expose); outbound limited to
443 (HTTPS), 7844 tcp+udp (cloudflared edge), and 53 (DNS).

> No VLAN capability? The weaker fallback is a second bridge with no LAN gateway
> + strict Proxmox firewall rules. A real VLAN with router enforcement is correct.

## In-container steps

```bash
pct enter 110
git clone <your repo URL> /srv/wfm/app        # Python scraper + prototype/
/srv/wfm/app/deploy/setup-container.sh         # installs Caddy, venv, units, upgrades
```

Then finish the manual bits the script prints: install `cloudflared` with your
tunnel token, drop the built `dist/` at `/srv/wfm/app/prototype/dist`, start the
first scrape, and verify headers + the companion fetch on the live URL.

---

## Build / deploy (`dist/`)

Build in CI (keeps node/bun and `node_modules` — a large supply-chain surface —
off the internet-facing box). The box only ever holds *built static output*.

```yaml
# .github/workflows/build-web.yml (sketch)
- uses: oven-sh/setup-bun@v2
- run: cd prototype && bun install --frozen-lockfile && bun run test && bun run build
- run: tar -C prototype/dist -czf dist.tgz .
- uses: softprops/action-gh-release@v2
  with: { files: dist.tgz, tag_name: web-${{ github.sha }} }
```

On the box this is automatic: [`wfm-web-pull.timer`](wfm-web-pull.timer) runs
[`pull-web.sh`](pull-web.sh) every 15 min, which checks the `web-latest`
release asset's `updated_at`, downloads on change, sanity-checks the tree
(index.html + assets/ present), swaps `dist/` atomically, and reloads Caddy.
Manual equivalent: extract to `/srv/wfm/app/prototype/dist` and
`systemctl reload caddy`. Rollback = re-extract the previous artifact. Don't worry about the `market.json` baked into `dist/` —
the Caddyfile serves the cron-refreshed one from `prototype/public/` instead.

Solo alternative: `bun run build` locally, then
`rsync -az --delete prototype/dist/ wfm@<tailscale-ip>:/srv/wfm/app/prototype/dist/`
over Tailscale (never an exposed port).

## Where market data comes from (bootstrap vs production)

The box is the **production writer**: `wfm-scrape.timer` runs the full
pipeline every 2 h and Caddy serves the box's own
`prototype/public/market.json`. The repo's committed copy is a **bootstrap
snapshot only**, refreshed daily by the `refresh-market` GH cron (offset so
the two scrapers never run concurrently): a fresh clone / brand-new box
starts with data at most a day old, and the box's first own scrape (within
2 h of `wfm-scrape.timer` enablement) takes over from there. If the repo
copy is ever badly stale, `workflow_dispatch` the cron manually — do NOT
scrape from two places at the same time.

### Live-site data stale? Triage in this order

1. `curl -s https://tennoworth.app/market.json | python3 -c 'import json,sys; print(json.load(sys.stdin)["updated_at"])'`
   — more than ~3 h old means the **box's** writer failed. The GH cron
   cannot fix this (bootstrap-only) — don't debug the Actions tab.
2. On the box: `systemctl status wfm-scrape.timer wfm-scrape.service` and
   `journalctl -u wfm-scrape.service -n 100`.
3. Known failure mode (bit us 2026-07-20): scrape runtime grows with the
   item catalog and crossed `TimeoutStartSec` — systemd killed every run
   ~200 s before it could write market.json, so the site silently served
   the last good snapshot for 14 h. If the journal shows healthy progress
   lines ending in `start operation timed out`, raise `TimeoutStartSec`
   (unit file lives in this dir; runtime was ~3800 s at 3837 items).
   Watch the same creep against the GH job's `timeout-minutes: 90` and the
   2 h timer cadence itself.

## Converter shadow (phase 2b)

The market pipeline is mid-migration from Python to Rust. Phase 2b runs the
**Rust converter in shadow** on the box to prove semantic parity before any
cutover — Python stays the production writer.

**What it does.** After each production scrape promotes `market.json`,
`run-scrape.sh` runs `wfm-scrape build` (the Rust port of
`csv_to_market_json.py`) in a throwaway dir against the *same*
`wfm_results.csv` and the *same* prior snapshot the Python run used, then
compares its output to the promoted `market.json` with
[`scripts/semantic_diff.py`](../scripts/semantic_diff.py). It **never**
touches the promoted output: the shadow writes only into a temp dir and the
whole step is wrapped so any failure (or a missing/broken binary) exits 0 —
a shadow crash cannot fail the scrape service. The step runs only when
`/srv/wfm/bin/wfm-scrape` exists, so GitHub Actions runs of the same script
skip it automatically.

The binary is delivered like the web bundle: CI publishes a rolling
`scrape-latest` release ([`build-scrape.yml`](../.github/workflows/build-scrape.yml)),
and [`wfm-scrape-pull.timer`](wfm-scrape-pull.timer) runs
[`pull-scrape.sh`](pull-scrape.sh) every 30 min to install it to
`/srv/wfm/bin` — the box never builds Rust.

**How to read it.** One summary line lands in the journal per scrape:

```bash
journalctl -u wfm-scrape.service | grep SHADOW
#   SHADOW PARITY OK (2026-07-19T…)                     ← converged this run
#   SHADOW PARITY DIFF: 3 paths — first: .items.foo.avg …  ← diverged
#   SHADOW UNAVAILABLE: rust build failed (rc=1, …)     ← shadow itself broke
```

The full diff for every run is appended to **`/srv/wfm/shadow-parity.log`**
(auto-trimmed to the last ~500 lines).

**Expect occasional benign DIFFs.** The two converters fetch upstreams
(warframestat / relics / vault / Baro) at slightly different instants, so a
surface that changed between the two fetches shows a transient diff that
clears on the next run. The cutover gate is **zero structural / persistent
diffs across an observation window**, not a single clean run.

**Cutover is a later, explicit step.** Once the log shows a clean window,
swapping Python for Rust as the production writer is a separate deliberate
change — this phase only observes.

---

## Admin access

Run **Tailscale** on the container (or the Proxmox host) and SSH/manage over the
tailnet, so port 22 is never exposed even to your LAN. Pattern: Cloudflare Tunnel
for the public app, Tailscale for the management plane. There is **no app auth
surface**, so fail2ban on this container buys nothing — skip it.

## Proxmox host hardening (essentials)

- Block 8006 + SSH from the internet **and** the DMZ VLAN (mgmt over Tailscale/LAN only).
- Enable **2FA (TOTP)** on `root@pam`.
- SSH key-only (`PasswordAuthentication no`). Don't fully disable root SSH if you
  ever cluster (PVE uses `root@node` SSH internally).
- Datacenter firewall default-deny inbound.
- **Keep the host kernel patched** — this is *the* mitigation for the LXC
  shared-kernel risk. If you find that unacceptable, swap the LXC for a minimal
  Debian VM; every config file here applies unchanged.

## Backups

`market.json` is regenerable (cron rebuilds it every 2h) — don't back it up.
Worth keeping: the repo (already in git) and this `deploy/` dir, so the whole box
is reproducible from git. Optionally a periodic Proxmox `vzdump` of the container.

## Threat model → containment

| Risk | Containment |
|---|---|
| Exposing Proxmox host/admin | App in unprivileged LXC on a VLAN firewall-denied to 8006/SSH and the LAN; cloudflared exposes only `localhost:8081`. A container compromise is a powerless host UID with no route to pivot. |
| Open inbound ports | **Zero** — cloudflared dials out; router firewall stays closed; scanners see nothing. SSH is Tailscale-only. |
| Proxy/TLS misconfig | TLS at Cloudflare's edge; Caddy serves loopback-only and re-applies the full header set (verifiable with `curl -I`). CSP is default-deny. |
| Build supply-chain | node/bun never touch the exposed box; CI builds, box serves static output; lockfile frozen, Actions SHA-pinned. |
| Half-written / truncated `market.json` | Atomic `os.replace` prevents *torn* files (readers get whole-old or whole-new). But a sustained 429 makes the scraper skip items and flush a *complete-but-truncated* CSV — atomicity doesn't catch that. `run-scrape.sh` adds a row-count floor (≥800 and ≥75% of prior) and refuses to rebuild from a gutted scrape, keeping the old snapshot. |
| WFM rate-limit (1015) | 3 req/s, 2h cadence + jitter, request timeouts. The scraper UA is now a real browser string (was a generic UA — fixed in-repo). |

## Two things to verify after deploy (couldn't be pre-confirmed)

1. **`https → http://127.0.0.1` companion fetch.** Browser loopback/mixed-content
   rules have tightened; the CSP allows it and Chromium's loopback carve-out
   *should* permit it, but **confirm in DevTools on the live HTTPS URL** that the
   companion fetch isn't blocked before assuming it works.
2. **Scraper from a residential IP.** Watch the first cron runs
   (`journalctl -u wfm-scrape.service -f`) for repeated 429/403. The browser UA
   fix should prevent the WFM 1015 block, but verify.
