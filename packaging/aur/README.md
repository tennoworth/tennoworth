# AUR packaging

Linux ships through the AUR rather than a bundle built in CI. The
`release-desktop.yml` workflow publishes **Windows installers + Linux deb/rpm +
the two AUR packages** — see the comment at the top of it for why the AppImage
was dropped (short version: it bundled ubuntu-22.04's WebKitGTK, which aborts
at EGL init against a rolling-release Mesa and paints a white window).

Because the AUR builds from source against the user's own system libraries,
that whole class of drift disappears. The tradeoff is that Tauri's in-app
updater — which only ever supported AppImage on Linux — does nothing here;
`update.rs` no-ops the check on Linux so users never see a banner whose Install
button could not work. `pacman -Syu` is the update path.

## The two packages

| Package | Source | Who it's for |
|---|---|---|
| `tennoworth/` | builds from the GitHub source tag | anyone; always compiles against current system libs |
| `tennoworth-bin/` | prebuilt tarball from the versioned release | people who don't want to compile Rust + a webview |

They `provides`/`conflicts` each other, so pacman treats them as alternatives.

`tennoworth-bin` ships a dynamically-linked executable and resolves
webkit2gtk/gtk3 from the system — it does **not** bundle libraries, which is
what separates it from the AppImage that failed. Its one weakness is that a
webkit2gtk soname bump breaks it until it is rebuilt and republished, whereas
the source package just recompiles. That is why both exist.

The CI binary is built on ubuntu-22.04 to keep its glibc floor at 2.35; the
workflow fails the release if a newer glibc symbol creeps in, because glibc is
backward- but not forward-compatible and the build host's version becomes the
floor for every user.

## Cutting a release

Each AUR package is its **own git repo** on `aur.archlinux.org`; these
directories are the source of truth that gets copied into them. Publishing is
**automated** — the `aur` job in `release-desktop.yml` runs on every
`desktop-v*` tag. Bumping a desktop release therefore needs no manual AUR step:

1. Cut the desktop release first. Both packages download from the tag —
   `tennoworth` the source tarball, `tennoworth-bin` the CI-built binary
   tarball — so `desktop-v<pkgver>` must exist:

   ```sh
   git tag -a desktop-v0.3.4 -m "TennoWorth desktop 0.3.4" && git push github desktop-v0.3.4
   ```

2. Make sure `pkgver` in both `PKGBUILD`s already matches the tag — the `aur`
   job's first gate fails the run if it doesn't. This is why the version bump
   commit touches them alongside `tauri.conf.json` / `Cargo.toml`.

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
   git commit -am "tennoworth 0.3.4" && git push
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
ssh aur@aur.archlinux.org addpubkey ~/.ssh/aur-ci.pub   # register on the AUR account
gh secret set AUR_SSH_PRIVATE_KEY -f ~/.ssh/aur-ci     # expose to the workflow
```

The job pins aur.archlinux.org's ed25519 host key rather than trusting on
first use.

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

`pkgver` is a **fifth** place carrying a version. The `aur` job's first gate
now covers it — the run fails if the tag and the two `PKGBUILD`s disagree — but
that only catches the mismatch at publish time. When bumping the desktop app,
keep these in step in the same commit:

| Where | Feeds |
|---|---|
| `desktop-v*` git tag | names the release, and the tarball this PKGBUILD fetches |
| `companion/tennoworth-desktop/tauri.conf.json` | `latest.json`'s version |
| `companion/tennoworth-desktop/Cargo.toml` | `CARGO_PKG_VERSION`, what the app reports |
| `prototype/package.json` | the version shown in the SPA sidebar |
| `packaging/aur/PKGBUILD` | the AUR package version |
