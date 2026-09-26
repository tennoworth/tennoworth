#!/usr/bin/env bash
# Provision the static-host container. Run INSIDE the unprivileged LXC as root
# (`pct enter <id>` from the Proxmox host), AFTER you've placed the repo at
# /srv/wfm/app. See the deploy runbook for the Proxmox-host steps (pct create,
# VLAN/firewall) that come first.
#
# Idempotent-ish: safe to re-run. Edit the CLOUDFLARED_TOKEN line or run the
# `cloudflared service install` step by hand.
#
# The pipeline itself is not installed here: scripts/deploy-scrape-host.sh owns
# the scrape binary, its driver script and the wfm-scrape units, and installs
# them from a reviewed revision. This script provisions the box around it.
set -euo pipefail

REPO=/srv/wfm/app          # the git repo (Rust pipeline binary + frontend/public + frontend/dist)
DEPLOY="$REPO/deploy"

if [[ ! -d "$REPO/frontend" ]]; then
  echo "ERROR: expected the repo at $REPO (with frontend/ and deploy/)." >&2
  echo "Clone it there first, then re-run." >&2
  exit 1
fi

echo "==> Base packages"
apt-get update
apt-get install -y debian-keyring debian-archive-keyring apt-transport-https curl gpg

echo "==> Caddy"
if ! command -v caddy >/dev/null; then
  curl -1sLf 'https://dl.cloudsmith.io/public/caddy/stable/gpg.key' \
    | gpg --dearmor -o /usr/share/keyrings/caddy-stable-archive-keyring.gpg
  curl -1sLf 'https://dl.cloudsmith.io/public/caddy/stable/debian.deb.txt' \
    > /etc/apt/sources.list.d/caddy-stable.list
  apt-get update && apt-get install -y caddy
fi

echo "==> Service user"
# wfm's home is a data directory rather than the deployment root. Root executes
# the scripts under /srv/wfm, so nothing root runs may sit beneath a directory
# the service user can write to - otherwise replacing a directory entry is
# enough to replace the program, however the file's own mode reads.
id wfm >/dev/null 2>&1 || useradd --system --create-home --home-dir /srv/wfm/data --shell /usr/sbin/nologin wfm
if [ "$(getent passwd wfm | cut -d: -f6)" != /srv/wfm/data ]; then
  # Installs that predate the split used /srv/wfm as the home. Re-point the
  # passwd entry only; no files move, and $HOME stays writable once the
  # deployment root becomes root-owned.
  usermod -d /srv/wfm/data wfm
fi
mkdir -p /srv/wfm/data
mkdir -p /srv/wfm/observations
install -m 0755 "$DEPLOY/pull-web.sh" /srv/wfm/pull-web.sh
install -m 0755 "$DEPLOY/pull-policy.sh" /srv/wfm/pull-policy.sh
mkdir -p /srv/wfm/policy
# Without this the checkout at /srv/wfm/app only moves when a human moves it,
# and the copies under /srv/wfm drift from the repo silently - the box ran a
# retired pipeline 19 commits behind main for a week that way.
install -m 0755 "$DEPLOY/pull-app.sh" /srv/wfm/pull-app.sh
# OnFailure handler for every unit below. Optional webhook config lives in
# /etc/wfm-alert.env (ALERT_WEBHOOK_URL=...); absent means log-only.
install -m 0755 "$DEPLOY/alert.sh" /srv/wfm/alert.sh
mkdir -p /srv/wfm/bin

# Root owns the deployment root and every script a root unit executes. A
# blanket `chown -R wfm:wfm /srv/wfm` made all of them replaceable through
# their parent directory, which is a direct path from the service account to
# root. Only the paths a service genuinely writes are handed over, and each is
# named rather than inherited from a recursive chown.
chown root:root /srv/wfm
chown -R root:root /srv/wfm/bin
chown root:root /srv/wfm/*.sh
chown wfm:wfm /srv/wfm/data
chown wfm:wfm /srv/wfm/observations
chmod 750 /srv/wfm/observations
# The policy puller runs as wfm (wfm-policy-pull.service) and publishes here.
chown -R wfm:wfm /srv/wfm/policy
# The scraper writes its outputs into the checkout and the git pullers move it,
# so that subtree stays writable by wfm. This is a KNOWN REMAINING ESCALATION
# PATH, not an oversight: root installs the executed copies from this tree, so
# until those installs are verified (SEC-3.4) or the pipeline writes to a
# dedicated data directory (SEC-3.6), a wfm-writable checkout is still a route
# to code root will run.
chown -R wfm:wfm "$REPO"

# The pullers run as root against a wfm-owned checkout, which git rejects as
# "dubious ownership" unless the path is vouched for. This exception is a
# symptom, not a fix: root should not be running git against a repository a
# lower-privileged account can rewrite (SEC-3.3). It stays only because the
# scraper still writes its outputs into the checkout; it can be removed once
# the pipeline writes to /srv/wfm/data and the checkout becomes root-owned.
# It must be --system: --global writes /root/.gitconfig, which git only finds
# via HOME, and systemd starts these units with no HOME set - so a --global
# exception works when you run the script by hand over ssh and fails the moment
# the timer fires it. safe.directory is also deliberately ignored from a repo's
# own local config. --add unconditionally would stack a duplicate line on every
# re-run; this script is meant to be safe to re-run.
git config --system --get-all safe.directory 2>/dev/null | grep -qx "$REPO" \
  || git config --system --add safe.directory "$REPO"

echo "==> Caddy config"
if [ -s /etc/caddy/Caddyfile ] && ! grep -q 'frontend/dist' /etc/caddy/Caddyfile; then
  # A non-empty Caddyfile that isn't ours = this box already serves other sites.
  # Overwriting it would 502 every other hostname on reload. Skip + instruct.
  cp -n /etc/caddy/Caddyfile /etc/caddy/Caddyfile.bak 2>/dev/null || true
  echo "    Existing /etc/caddy/Caddyfile detected (backed up to .bak) - NOT overwriting."
  echo "    Paste the site block from $DEPLOY/Caddyfile into your config, pick a free"
  echo "    localhost port, point a tunnel hostname at it, then: systemctl reload caddy"
else
  install -m 0644 "$DEPLOY/Caddyfile" /etc/caddy/Caddyfile
  caddy validate --config /etc/caddy/Caddyfile
  systemctl enable --now caddy
  systemctl reload caddy
fi

install -m 0644 "$DEPLOY/wfm-policy-pull.service" /etc/systemd/system/wfm-policy-pull.service
install -m 0644 "$DEPLOY/wfm-policy-pull.timer" /etc/systemd/system/wfm-policy-pull.timer
# Enable after installing the verifier with the dedicated public key.
if [ -x /srv/wfm/bin/wfm-policy ]; then
  systemctl daemon-reload
  systemctl enable --now wfm-policy-pull.timer
fi

# The scrape units come with the binary they execute, from
# scripts/deploy-scrape-host.sh; provisioning the box never creates half that
# set on its own.
echo "==> App-pull + web-pull timers"
install -m 0644 "$DEPLOY/wfm-alert@.service"      /etc/systemd/system/wfm-alert@.service
install -m 0644 "$DEPLOY/wfm-app-pull.service"    /etc/systemd/system/wfm-app-pull.service
install -m 0644 "$DEPLOY/wfm-app-pull.timer"      /etc/systemd/system/wfm-app-pull.timer
install -m 0644 "$DEPLOY/wfm-web-pull.service"    /etc/systemd/system/wfm-web-pull.service
install -m 0644 "$DEPLOY/wfm-web-pull.timer"      /etc/systemd/system/wfm-web-pull.timer
systemctl daemon-reload
systemctl enable --now wfm-app-pull.timer wfm-web-pull.timer

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

2. The built site arrives without a build here (keep node/bun off the exposed
   box): wfm-web-pull.timer, enabled above, fetches the web-latest release
   asset into $REPO/frontend/dist. Start it once to populate it now:
     systemctl start wfm-web-pull.service

3. Deploy the scrape pipeline from the maintainer checkout
   (scripts/deploy-scrape-host.sh). It installs the binary, its driver script
   and the wfm-scrape units, enables the sweep timer, and refuses a revision
   that is not reviewed and clean. Kick a first sweep here:
     systemctl start wfm-scrape.service
     journalctl -u wfm-scrape.service -f
   Watch for repeated 429/403 (WFM 1015). The UA is a descriptive project
   string - the form WFM's rules require and the only one verified accepted -
   so this should be fine from a residential IP, but verify.

4. Verify the hosted page and data headers on the live HTTPS URL:
     curl -sI https://wfm.yourdomain.com | grep -iE 'strict-transport|frame-options|content-security'
   Then open the page in a browser, search for an item, and confirm in
   DevTools that the page loads with no CSP violations.
NEXT
