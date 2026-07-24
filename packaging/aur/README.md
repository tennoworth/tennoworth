# AUR packaging

Linux ships through the AUR rather than a bundle built in CI. The
`release-desktop.yml` workflow publishes **Windows installers only** — see the
comment at the top of it for why the AppImage was dropped (short version: it
bundled ubuntu-22.04's WebKitGTK, which aborts at EGL init against a
rolling-release Mesa and paints a white window).

Because the AUR builds from source against the user's own system libraries,
that whole class of drift disappears. The tradeoff is that Tauri's in-app
updater — which only ever supported AppImage on Linux — does nothing here;
`update.rs` no-ops the check on Linux so users never see a banner whose Install
button could not work. `pacman -Syu` is the update path.

## Cutting a release

The AUR package is a **separate git repo** hosted on `aur.archlinux.org`; this
directory is the source of truth that gets copied into it. Publishing needs an
AUR account with an SSH key registered, so it is a manual step.

1. Cut the desktop release first — the PKGBUILD downloads the GitHub tag
   tarball, so `desktop-v<pkgver>` must already exist:

   ```sh
   git tag -a desktop-v0.3.1 -m "TennoWorth desktop 0.3.1" && git push github desktop-v0.3.1
   ```

2. Update `pkgver` in `PKGBUILD` to match, then replace the placeholder
   checksum with the real one (`sha256sums=('SKIP')` is only a placeholder so
   the file parses before the tag exists — never publish with SKIP):

   ```sh
   cd packaging/aur
   updpkgsums
   ```

3. Build and install it locally before pushing. This is the only real test of
   the package — CI never builds a Linux bundle:

   ```sh
   makepkg -si
   ```

4. Regenerate `.SRCINFO` (the AUR rejects a push whose `.SRCINFO` disagrees
   with the `PKGBUILD`) and push to the AUR repo:

   ```sh
   makepkg --printsrcinfo > .SRCINFO
   # in the aur.archlinux.org clone:
   git commit -am "tennoworth 0.3.1" && git push
   ```

`.SRCINFO` is deliberately not committed here — it is per-version generated
output, and a stale copy in this repo would be worse than none.

## Version sources

`pkgver` is a **fifth** place carrying a version, and unlike the other four it
is not covered by `release-desktop.yml`'s guard. When bumping the desktop app,
keep these in step:

| Where | Feeds |
|---|---|
| `desktop-v*` git tag | names the release, and the tarball this PKGBUILD fetches |
| `companion/tennoworth-desktop/tauri.conf.json` | `latest.json`'s version |
| `companion/tennoworth-desktop/Cargo.toml` | `CARGO_PKG_VERSION`, what the app reports |
| `prototype/package.json` | the version shown in the SPA sidebar |
| `packaging/aur/PKGBUILD` | the AUR package version |
