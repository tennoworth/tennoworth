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
  A local binary is offered both `setcap cap_sys_ptrace=eip` and
  `ptrace_scope=0`, because neither is narrower: the capability is held by one
  binary but can trace any process, root's included, while scope 0 reaches only
  the user's own processes but opens all of them to each other. Scope 3
  disables ptrace even for a capable binary.
- **`CAP_SYS_PTRACE` is effective only around the two opens.** `capability.rs`
  clears it from the effective set (keeping it permitted) first thing in the
  desktop's `main`, before any thread exists, and `PtraceRaised` raises it on
  the scanning thread for the `/proc/<pid>/maps` and `/proc/<pid>/mem` opens
  only; the kernel checks access at open. Capability sets are per thread, so a
  new early thread or a scan moved across threads breaks this. It stops code
  running in the app from using the capability by accident, not code injected
  into it, which can raise a permitted capability again - and GTK still loads
  modules named in the user's own `settings.ini` under a file capability. In
  secure-execution mode startup also removes the module-loading variables
  listed in `MODULE_LOADING_VARS`, each checked against its library's source.
  Without a permitted capability all of this is a silent no-op.
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
