#!/usr/bin/env bash
# Provision the static-host container. Run INSIDE the unprivileged LXC as root
# (`pct enter <id>` from the Proxmox host), AFTER you've placed the repo at
# /srv/wfm/app. See deploy/README.md for the Proxmox-host steps (pct create,
# VLAN/firewall) that come first.
#
# Idempotent-ish: safe to re-run. Edit the CLOUDFLARED_TOKEN line or run the
# `cloudflared service install` step by hand.
set -euo pipefail

REPO=/srv/wfm/app          # the git repo (Python scraper + prototype/public + prototype/dist)
DEPLOY="$REPO/deploy"

if [[ ! -d "$REPO/prototype" ]]; then
  echo "ERROR: expected the repo at $REPO (with prototype/, wfm_demand.py, scripts/)." >&2
  echo "Clone it there first, then re-run." >&2
  exit 1
fi

echo "==> Base packages"
apt-get update
apt-get install -y debian-keyring debian-archive-keyring apt-transport-https curl gpg python3 python3-venv

echo "==> Caddy"
if ! command -v caddy >/dev/null; then
  curl -1sLf 'https://dl.cloudsmith.io/public/caddy/stable/gpg.key' \
    | gpg --dearmor -o /usr/share/keyrings/caddy-stable-archive-keyring.gpg
  curl -1sLf 'https://dl.cloudsmith.io/public/caddy/stable/debian.deb.txt' \
    > /etc/apt/sources.list.d/caddy-stable.list
  apt-get update && apt-get install -y caddy
fi

echo "==> Service user + Python venv"
id wfm >/dev/null 2>&1 || useradd --system --create-home --home-dir /srv/wfm --shell /usr/sbin/nologin wfm
python3 -m venv /srv/wfm/venv
/srv/wfm/venv/bin/pip install --upgrade pip requests
install -m 0755 "$DEPLOY/run-scrape.sh" /srv/wfm/run-scrape.sh
install -m 0755 "$DEPLOY/pull-web.sh" /srv/wfm/pull-web.sh
chown -R wfm:wfm /srv/wfm

echo "==> Caddy config"
if [ -s /etc/caddy/Caddyfile ] && ! grep -q 'prototype/dist' /etc/caddy/Caddyfile; then
  # A non-empty Caddyfile that isn't ours = this box already serves other sites.
  # Overwriting it would 502 every other hostname on reload. Skip + instruct.
  cp -n /etc/caddy/Caddyfile /etc/caddy/Caddyfile.bak 2>/dev/null || true
  echo "    Existing /etc/caddy/Caddyfile detected (backed up to .bak) — NOT overwriting."
  echo "    Paste the site block from $DEPLOY/Caddyfile into your config, pick a free"
  echo "    localhost port, point a tunnel hostname at it, then: systemctl reload caddy"
else
  install -m 0644 "$DEPLOY/Caddyfile" /etc/caddy/Caddyfile
  caddy validate --config /etc/caddy/Caddyfile
  systemctl enable --now caddy
  systemctl reload caddy
fi

echo "==> Scrape + web-pull timers"
install -m 0644 "$DEPLOY/wfm-scrape.service"   /etc/systemd/system/wfm-scrape.service
install -m 0644 "$DEPLOY/wfm-scrape.timer"     /etc/systemd/system/wfm-scrape.timer
install -m 0644 "$DEPLOY/wfm-web-pull.service" /etc/systemd/system/wfm-web-pull.service
install -m 0644 "$DEPLOY/wfm-web-pull.timer"   /etc/systemd/system/wfm-web-pull.timer
systemctl daemon-reload
systemctl enable --now wfm-scrape.timer wfm-web-pull.timer

echo "==> Unattended security upgrades"
apt-get install -y unattended-upgrades
dpkg-reconfigure -plow unattended-upgrades || true

cat <<'NEXT'

==> Done with the local provisioning. Remaining steps (manual):

1. cloudflared (zero inbound ports). In the Cloudflare Zero Trust dashboard:
   Networks -> Tunnels -> Create tunnel "wfm-web" -> public hostname
   wfm.yourdomain.com -> http://localhost:8081 (match the Caddyfile port). Then here:

     curl -L -o cloudflared.deb \
       https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-linux-amd64.deb
     apt-get install -y ./cloudflared.deb && rm cloudflared.deb
     cloudflared service install <YOUR_TUNNEL_TOKEN>
     systemctl status cloudflared

2. Get the built site onto the box (do NOT build here — keep node/bun off the
   exposed box). From CI or your dev machine, place the Vite build at
   $REPO/prototype/dist  (see deploy/README.md "Build / deploy").

3. Kick a first scrape and watch it:
     systemctl start wfm-scrape.service
     journalctl -u wfm-scrape.service -f
   Watch for repeated 429/403 (WFM 1015). The UA is now a real browser string,
   so this should be fine from a residential IP — but verify.

4. Verify headers + the companion fetch on the LIVE https URL:
     curl -sI https://wfm.yourdomain.com | grep -iE 'strict-transport|frame-options|content-security'
   Then open the page in a browser, connect the companion, and confirm in
   DevTools that the fetch to http://127.0.0.1:* is NOT blocked as mixed content.
NEXT
