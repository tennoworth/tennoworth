# game-acquisition/ - process detection, memory scan, DE inventory

Parent rules: [`../AGENTS.md`](../AGENTS.md). This crate never talks to
warframe.market; the DE inventory request is its only network call and is
classified in `wfm-client/tests/send_sites.rs`.

## Cross-platform memory access

`scan_session(pid)` is implemented twice, gated by
`#[cfg(target_os = …)]`:

- **Linux**: parse `/proc/<pid>/maps` → seek+read `/proc/<pid>/mem` in
  chunks with a small overlap so cross-chunk pattern matches don't escape.
  AppImage runs cannot retain file capabilities on their temporary nosuid
  mount, so `ptrace_open_error()` directs them to adjust Yama's ptrace scope.
  A local binary can instead use the tighter one-time
  `setcap cap_sys_ptrace=eip` grant. Scope 3 disables ptrace even for a
  capable binary.
- **Windows**: `VirtualQueryEx` to walk regions, `ReadProcessMemory`
  to read, filtering on `MEM_COMMIT` and excluding `PAGE_NOACCESS` /
  `PAGE_GUARD`. No elevation needed if running as the user that
  launched WF.

Patterns scanned (`regex::bytes::Regex`):
- `accountId=([0-9a-fA-F]{24})&nonce=([0-9]{6,})` - session creds
- `"BuildLabel":"([0-9.]+)/[A-Za-z0-9]+` - game build → appVersion
- `&ct=([A-Z]{2,4})\b` - platform tag

The scanned values are session secrets: see "The app never prints secrets" in
[`../AGENTS.md`](../AGENTS.md).
