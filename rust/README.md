# rust/ - Rust workspace

The Rust side of TennoWorth. The product is the **desktop app**
(`tennoworth-desktop`, a Tauri v2 shell) driving `wfm-core` over IPC; the
host pipeline crate `wfm-scrape` feeds the market snapshot. The standalone
CLI companion (`wfm-fetch-inventory` with `fetch`/`login`/`serve`) was
removed on 2026-08-02 - the desktop app replaced it.

## Crates

Besides the desktop app, its core and the pipeline, the workspace holds the
shared market calculations and transport, the browser preview's WebAssembly
build, and the host services: usage and price reports, and the private
announcement publisher. The [architecture guide's repository map](../docs/architecture.md#repository-map)
lists every crate and what it owns; [price reports](../docs/price-reports.md)
documents that service's privacy contract.

See [CONTRIBUTING.md](../CONTRIBUTING.md#native-desktop-development) for
Windows and Linux prerequisites and build commands. Build the desktop frontend
before compiling the app: Cargo embeds that bundle. The
[architecture guide](../docs/architecture.md) maps native services, persistence,
capture, and pipeline stages.

## Ban risk

The desktop app only ever *reads* memory - it never writes to the game, never
injects code, and doesn't interact with anti-cheat. **We cannot promise this
is ban-safe.** Sainan's
[warframe-api-helper](https://github.com/Sainan/warframe-api-helper) has used
the same read-only approach for years with no documented bans, but DE has never
formally blessed this category of tool. **Use at your own risk; there is no
warranty.**

## License

MIT.
