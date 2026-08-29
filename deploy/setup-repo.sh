#!/bin/sh
# RETIRED - do not run this on a new box.
#
# One-time bootstrap for the signed apt + dnf repositories. Linux is
# AppImage-only as of the release after 0.5.0, so no new deb or rpm is ever
# built and this script has nothing to bootstrap for. It is kept, unmodified
# below this header, for exactly two reasons:
#
#   1. The repositories it created are STILL SERVED, frozen at their last
#      published version, so that nobody who ran `apt install tennoworth` gets
#      a 404 or a broken index. This file is the only record of how that tree
#      is laid out, what `SignWith` key it uses, and what the served
#      tennoworth.repo says - all of which you need to reason about the frozen
#      repos or to eventually take them down cleanly.
#   2. The GPG-handling notes in it (the /etc/rpm/macros discovery, the
#      passphrase probe) were expensive to find and are worth keeping.
#
# The publisher that fed these repos, deploy/pull-packages.sh, is now a no-op
# stub - see its header. SECURITY.md documents the frozen repos and the key.
#
# Why the box signed rather than CI: a repo-signing key in GitHub Actions
# secrets would let anyone who compromises the workflow serve trusted packages
# to every user who added our repo. The private subkey lives only here; the
# primary key never left the maintainer's laptop.
set -eu

FPR=CC5F8E297E446C5B8D2769AC64093BF63D573CE8
REPO=/srv/wfm/repo

# rpm is Debian's package for `rpmsign` - dnf users expect gpgcheck=1, which
# needs the .rpm files themselves signed, not just the metadata.
apt-get update
apt-get install -y --no-install-recommends reprepro createrepo-c rpm gnupg

# The signing key must already be here; failing now with a clear message beats
# reprepro failing later with "no secret key".
if ! gpg --list-secret-keys "$FPR" >/dev/null 2>&1; then
  echo "ERROR: signing subkey $FPR is not in root's keyring." >&2
  echo "Import it first - see the deploy runbook." >&2
  exit 1
fi

# Unattended signing must not prompt. A passphrase-protected key here would
# make the timer hang rather than fail, which is much harder to notice.
echo test > /tmp/.sigprobe
if ! gpg --batch --yes --pinentry-mode error --detach-sign -o /tmp/.sigprobe.sig /tmp/.sigprobe 2>/dev/null; then
  echo "ERROR: the signing key needs a passphrase, so unattended signing will hang." >&2
  echo "Strip it from THIS copy: gpg --edit-key $FPR -> passwd -> empty -> save" >&2
  rm -f /tmp/.sigprobe
  exit 1
fi
rm -f /tmp/.sigprobe /tmp/.sigprobe.sig

mkdir -p "$REPO/apt/conf" "$REPO/rpm"

# Codename `stable` rather than a Debian suite name (bookworm/trixie): this
# repo ships one architecture-independent-of-distro package, and pinning a
# suite name would imply per-suite builds we do not produce. Users get
# `deb ... stable main` on Debian and Ubuntu alike.
cat > "$REPO/apt/conf/distributions" <<EOF
Origin: TennoWorth
Label: TennoWorth
Suite: stable
Codename: stable
Architectures: amd64
Components: main
Description: TennoWorth - Warframe inventory + market dashboard
SignWith: $FPR
EOF

cat > "$REPO/apt/conf/options" <<'EOF'
verbose
basedir /srv/wfm/repo/apt
EOF

# %_gpg_name is how `rpm --addsign` picks a key; without it rpmsign fails with
# an unhelpful "Could not exec gpg" (or, on this rpm build, `You must set
# "%_gpg_name" in your macro file`).
#
# It goes in /etc/rpm/macros, NOT /root/.rpmmacros: rpm only finds ~/.rpmmacros
# through HOME, and wfm-repo-pull.service runs with no HOME set. That is not
# theoretical - the box had exactly this file and the dnf repo still sat 9 days
# stale on an unsigned-by-the-new-key build, because every publish since the
# 0.3.5 release failed at the signing step while `bash` runs of the same script
# worked. Same shape as the git safe.directory exception in setup-container.sh.
#
# /etc/rpm/macros.d/ is NOT read by the rpm in Debian stable - verified on the
# box, the macro stayed UNSET from there. Use the flat file.
mkdir -p /etc/rpm
cat > /etc/rpm/macros <<EOF
%_gpg_name $FPR
%_gpg_path /root/.gnupg
EOF
chmod 0644 /etc/rpm/macros

# The .repo file dnf users install. gpgcheck=1 verifies each package's own
# signature; repo_gpgcheck=1 verifies the signed repomd.xml on top, so a
# tampered index is caught even before a package is fetched.
cat > "$REPO/rpm/tennoworth.repo" <<'EOF'
[tennoworth]
name=TennoWorth
baseurl=https://tennoworth.app/rpm
enabled=1
gpgcheck=1
repo_gpgcheck=1
gpgkey=https://tennoworth.app/tennoworth-archive-keyring.asc
EOF

chown -R wfm:wfm "$REPO"
# reprepro and createrepo run as root from the timer; wfm owns the tree so
# Caddy (running as wfm) can read it.
chmod -R a+rX "$REPO"

echo "Repo skeleton ready at $REPO"
echo "NOTE: this script is retired - there is no longer a package to publish"
echo "into it. Linux ships as an AppImage only."
