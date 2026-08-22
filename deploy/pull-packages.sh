#!/bin/sh
# RETIRED — the apt/dnf publisher. Kept as a no-op stub on purpose.
#
# Linux is AppImage-only as of the release after 0.5.0: the desktop release
# workflow no longer builds a .deb or a .rpm, so there is nothing left for this
# script to fetch. It used to pull the CI-built packages for the newest
# desktop-v* tag, sign them, and republish /srv/wfm/repo via reprepro +
# createrepo_c.
#
# Why a stub instead of `git rm`:
#
#   wfm-repo-pull.timer is ENABLED on the box and fires every 30 minutes,
#   executing the COPY at /srv/wfm/pull-packages.sh. Deleting this file would
#   leave that copy in place (pull-app.sh only reinstalls files that exist in
#   the repo), and the old copy would then `curl -fsSL .../tennoworth-desktop-
#   amd64.deb` against a release that no longer carries one, fail, and fire
#   wfm-alert@ every half hour forever. pull-app.sh reinstalls any deploy/*.sh
#   that differs from its installed copy, so shipping THIS file makes the box
#   heal itself on the next app pull, with no SSH and no hand-editing.
#
# What is NOT retired: the repositories themselves. /srv/wfm/repo keeps its
# pool, its signed indexes and its .repo file, and the Caddyfile keeps serving
# /apt and /rpm. Anyone who already ran `apt install tennoworth` keeps a valid,
# signed, working repository — it is simply frozen at its last published
# version and will never offer another. They should switch to the AppImage; see
# README.md and SECURITY.md.
#
# To stop the timer entirely (optional — this stub is already harmless):
#   systemctl disable --now wfm-repo-pull.timer
set -eu

echo "pull-packages.sh is retired: Linux ships as an AppImage only, and desktop"
echo "releases no longer carry a .deb or .rpm. The apt/dnf repositories under"
echo "/srv/wfm/repo stay served, frozen at their last published version."
echo "Disable this timer when convenient: systemctl disable --now wfm-repo-pull.timer"
exit 0
