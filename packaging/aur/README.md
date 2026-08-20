# AUR packaging

The AUR is one of four Linux channels. `release-desktop.yml` publishes
**Windows installers + the Linux deb/rpm + an AppImage + the raw binary
tarball**, and pushes the two AUR packages.

**The AppImage is back.** This file used to say it had been dropped; that was
true for about a month. The first one (2026-07) was withdrawn for `Could not
create default EGL display: EGL_BAD_PARAMETER` on rolling-release Mesa, and
the cause was found on 2026-08-20: linuxdeploy bundles ubuntu-22.04's
libwayland client/cursor/egl/server, and the host's Wayland-EGL platform
rejects that 2022 client. WebKitGTK was never the problem. The release build
now strips those four libraries and repacks, so the AppImage resolves
libwayland from the host — and the same bundle runs cleanly. The repack
invalidates the build-time updater signature, so the file is re-signed
afterwards from the repacked bytes.

Because the AUR builds from source against the user's own system libraries,
that whole class of drift cannot happen here at all — which is why the source
package remains the recommendation for rolling distros.

The in-app updater is AppImage-only on Linux, which is exactly what
`update.rs`'s `updates_owned_by_packager()` encodes: running as an AppImage,
the check is real and self-update works through the same signed feed as
Windows; installed from pacman, apt or dnf, the check returns early so users
never see a banner whose Install button could not work. `pacman -Syu` is the
update path for both AUR packages.

## The two packages

| Package | Source | Who it's for |
|---|---|---|
| `tennoworth/` | builds from the GitHub source tag | anyone; always compiles against current system libs |
| `tennoworth-bin/` | prebuilt tarball from the versioned release | people who don't want to compile Rust + a webview |

They `provides`/`conflicts` each other, so pacman treats them as alternatives.

`tennoworth-bin` ships a dynamically-linked executable and resolves
webkit2gtk/gtk3 from the system — it does **not** bundle libraries, which is
what separated it from the AppImage back when the AppImage was broken. Its one
weakness is that a webkit2gtk soname bump breaks it until it is rebuilt and
republished, whereas the source package just recompiles. That is why both
exist.

The CI binary is built on ubuntu-22.04 to keep its glibc floor at 2.35; the
workflow fails the release if a newer glibc symbol creeps in, because glibc is
backward- but not forward-compatible and the build host's version becomes the
floor for every user.

## Cutting a release

Each AUR package is its **own git repo** on `aur.archlinux.org`; these
directories are the source of truth that gets copied into them. Publishing is
**automated** — the `aur` job in `release-desktop.yml` runs at the end of every
release. Bumping a desktop release therefore needs no manual AUR step:

1. Bump every version pin in one commit, with the tool that owns them:

   ```sh
   bun scripts/release.ts prepare minor     # or patch, major, or an X.Y.Z
   # fill in the CHANGELOG section it opened, then:
   git add -A && git commit -m "desktop 0.4.0"
   ```

   That writes `companion/tennoworth-desktop/Cargo.toml` (the authoritative
   version), refreshes `companion/Cargo.lock`'s own entry, and sets `pkgver` in
   both `PKGBUILD`s. `bun scripts/release.ts check` runs on every PR and fails
   on drift, so a half-done bump cannot reach `main`.

   The `Cargo.lock` half is not bookkeeping: it records tennoworth-desktop's
   own version, and the `tennoworth` package builds the tag's tarball with
   `cargo build --frozen`, which aborts rather than re-resolve a lock. 0.3.5
   and 0.3.6 both shipped without it and were unbuildable from source.

2. Merge to `main`, then cut the release: **Actions → release-desktop → Run
   workflow**, from `main`, with the same version. There is no tag to push —
   the workflow creates `desktop-v<version>` itself, at the commit it built,
   and only after every artifact has been produced and verified. Both packages
   download from that tag (`tennoworth` the source tarball, `tennoworth-bin`
   the CI-built binary tarball), which is why the `aur` job runs after the
   release is public rather than beside it.

3. The `aur` job then, per package: clones the AUR repo, copies these files in,
   replaces the `sha256sums=('SKIP')` placeholder with the checksum computed
   from the tag's actual artifacts (the `-bin` hash is cross-checked against
   the `.sha256` the release itself published), regenerates `.SRCINFO` in a
   throwaway Arch container (a hand-written one is a drift trap the AUR rejects
   only after the push lands), and pushes. If the `AUR_SSH_PRIVATE_KEY` secret
   is absent the job prints a notice and the release still ships — it never
   fails a desktop build over the AUR.

   The repo's copies keep `sha256sums=('SKIP')`: it exists only so the file
   parses before the tag does. Only the AUR copy receives a real checksum.

### Manual publish (rollback path)

If the `aur` job is broken or the maintainer needs to push by hand:

1. Clone the AUR repos and sync these directories in (this step is exactly
   what the job automates):

   ```sh
   git clone ssh://aur@aur.archlinux.org/tennoworth.git
   git clone ssh://aur@aur.archlinux.org/tennoworth-bin.git
   ```

2. Replace the placeholder checksum with the real one and build locally before
   pushing. CI never builds a Linux *package*, so this is the only real test
   either one gets:

   ```sh
   cd packaging/aur/tennoworth      && updpkgsums && makepkg -si
   cd ../tennoworth-bin             && updpkgsums && makepkg -si
   ```

3. Regenerate `.SRCINFO` (the AUR rejects a push whose `.SRCINFO` disagrees
   with its `PKGBUILD`) and push to the AUR repo:

   ```sh
   makepkg --printsrcinfo > .SRCINFO
   # in the aur.archlinux.org clone:
   git commit -am "tennoworth 0.3.7" && git push
   ```

`.SRCINFO` is deliberately not committed here — it is per-version generated
output, and a stale copy in this repo would be worse than none.

### The automation's key

`AUR_SSH_PRIVATE_KEY` is the private half of a dedicated keypair registered to
the maintainer's AUR account (the AUR authenticates per account over SSH, so
the key must belong to whoever maintains the packages). Use a key **separate
from your daily one** — it lives in GitHub repo secrets and only ever talks to
the AUR.

To set it up once:

```sh
ssh-keygen -t ed25519 -f ~/.ssh/aur-ci -N '' -C 'tennoworth CI'
ssh aur@aur.archlinux.org addpubkey < ~/.ssh/aur-ci.pub   # register on the AUR account
gh secret set AUR_SSH_PRIVATE_KEY --body "$(cat ~/.ssh/aur-ci)"   # expose to the workflow
```

(`gh secret set -f` would parse the key as an env file and choke on its
`-----BEGIN` line — `--body` takes the raw value instead.) Note that `$(cat …)`
strips the key's trailing newline, and an SSH PEM without it fails to load with
`error in libcrypto`. The workflow writes the secret back with a trailing
newline (`printf '%s\n'`) so it parses regardless — but verify after setting
it: `gh secret set` echoes nothing, so re-read it with
`gh secret list` and confirm a release run's `aur` job shows
"ssh-keygen -lf" succeeding rather than the libcrypto error. The job pins
aur.archlinux.org's ed25519 host key rather than trusting on first use.

## First-time AUR setup

One-off, and only the account holder can do it:

1. Register at <https://aur.archlinux.org/register/>, pasting `~/.ssh/id_*.pub`
   into the SSH Public Key field.
2. Verify: `ssh aur@aur.archlinux.org help` should list commands rather than
   `Permission denied (publickey)`.
3. Clone the (initially empty) repos — the name in the URL is what reserves
   the package:

   ```sh
   git clone ssh://aur@aur.archlinux.org/tennoworth.git
   git clone ssh://aur@aur.archlinux.org/tennoworth-bin.git
   ```

4. Copy this directory's `PKGBUILD` (+ `tennoworth.desktop` for the source
   package) in, add `.SRCINFO`, commit, push. The first push creates the
   package page.

## Version sources

There used to be six of these, kept in step by hand, with a release-time guard
as the only enforcement — so drift was always found while shipping. There are
now four, one of them authoritative and the other three derived:

| Where | Authority | Feeds |
|---|---|---|
| `companion/tennoworth-desktop/Cargo.toml` | **authoritative** | `CARGO_PKG_VERSION` — what the app reports, what the updater compares against, and (with no `version` key in `tauri.conf.json`) what Tauri writes into the bundle, the installer filenames and `latest.json` |
| `companion/Cargo.lock` | derived | nothing at runtime — but `cargo build --frozen` in this PKGBUILD aborts if it disagrees with the manifest, which is how 0.3.5 and 0.3.6 shipped unbuildable from source |
| `packaging/aur/tennoworth/PKGBUILD` | derived | the AUR source package version |
| `packaging/aur/tennoworth-bin/PKGBUILD` | derived | the AUR binary package version |

`bun scripts/release.ts prepare <bump>` writes all four. `bun
scripts/release.ts check` verifies them and runs on every PR (`version-pins`
in `audit.yml`) and again in the release workflow's preflight, so drift fails a
PR rather than a release.

The `desktop-v*` tag is no longer a version source: the release workflow
creates it from the version it was dispatched with, after confirming that
version matches all four pins.

Two pins were **removed** rather than automated. `tauri.conf.json`'s `version`
was pure duplication (Tauri v2 falls back to the Cargo package version when
the key is absent). `prototype/package.json`'s `version` was read by nothing —
the package is private and `vite.config.ts` bakes the build commit, not a
version, because the web app ships continuously and a desktop version pinned
to it would be a lie about when the build was made. It was the pin that
drifted to 0.3.3 under a 0.3.6 desktop.
