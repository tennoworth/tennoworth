#!/bin/sh
# Grant the one capability the inventory scan needs. Referenced by
# tauri.conf.json's bundle.linux.{deb,rpm}.postInstallScript; the AUR
# equivalent is packaging/aur/tennoworth/tennoworth.install, which must stay
# behaviourally identical.
#
# Why this exists: TennoWorth reads your inventory out of the running game's
# memory (/proc/<pid>/mem), which the kernel gates behind PTRACE_MODE_ATTACH.
# On the usual desktop default (kernel.yama.ptrace_scope=1) only a process's
# own descendants may attach, and the game is a child of Steam, not of us — so
# without CAP_SYS_PTRACE the very first scan fails. Doing it here means a GUI
# user never gets told to open a terminal after a failure.
#
# This runs again on every upgrade because replacing the binary clears its file
# capabilities. That is kernel behaviour, not a packaging quirk.
#
# Silent on success by design. It only speaks up when it could NOT do its job,
# because that is the case where the user has to act.

set -e

BIN=/usr/bin/tennoworth-desktop

# Nothing to do if the layout is not what we expect — never fail an install
# over this; the app degrades to its own runtime error, which is actionable.
[ -x "$BIN" ] || exit 0

if ! command -v setcap >/dev/null 2>&1; then
  echo "TennoWorth: 'setcap' not found, so inventory scanning will fail." >&2
  echo "  Install it (libcap2-bin on Debian/Ubuntu, libcap on Fedora), then:" >&2
  echo "    sudo setcap cap_sys_ptrace=eip $BIN" >&2
  exit 0
fi

# Fails on filesystems without xattr support (some containers, NFS homes).
if ! setcap cap_sys_ptrace=eip "$BIN" 2>/dev/null; then
  echo "TennoWorth: could not set cap_sys_ptrace on $BIN." >&2
  echo "  Inventory scanning will fail until this succeeds:" >&2
  echo "    sudo setcap cap_sys_ptrace=eip $BIN" >&2
fi

exit 0
