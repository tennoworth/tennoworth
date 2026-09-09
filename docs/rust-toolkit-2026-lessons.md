# 2026 Rust toolkit decisions

This note records which recommendations from
[My 2026 Rust Toolkit](https://www.namtao.com/rust-toolkit-2026/) were adopted
and which were measured and rejected.

## Adopted

| Lesson | Where it landed |
|---|---|
| Deny-panics Clippy policy | `rust/Cargo.toml` `[workspace.lints.clippy]`, `rust/clippy.toml`, and the `cargo-clippy` job feeding `audit-gate` |
| Local, reasoned lint exceptions | Sites unreachable by construction carry `#[allow(<lint>, reason = "...")]` beside the invariant |
| `cargo shear` | Local dead-dependency sweep; the Rust counterpart to `knip` |
| `cargo nextest` | Faster local test runner; `cargo test` remains the CI command |
| `cargo info` before `cargo add` | Dependency review checks license, Rust floor, and feature surface before adoption |
| Bacon | Optional local Clippy loop; fix the first compiler diagnostic first |

## Local verification

Run one ordinary workspace test pass from `rust/`:

```sh
cargo nextest run --workspace
cargo test --workspace --doc
```

Nextest does not run doctests; the second command is required. When nextest
is unavailable, use `cargo test --workspace` instead of both commands.
CI continues to use Cargo's test runner. Keep Clippy, `cargo shear`,
`cargo audit --deny warnings`, the selected ignored OCR regression and real
application probes as separate checks. The ordinary workspace pass includes
the scraper's fixtures against Cargo's freshly built binary.

For the selected capture regression:

```sh
cargo test -p tennoworth-desktop real_three_reward_capture_survives_common_display_shapes -- --ignored
```

Use one absolute, task-specific `CARGO_TARGET_DIR` and `TMPDIR` on the home
filesystem throughout an isolated task, including subsequent revisions.
Never share writable build directories between concurrent tasks. `/tmp` may
be a small tmpfs; linker outputs and test temporary files can exhaust it.
Build `frontend/dist-desktop` before plain Cargo desktop builds. The Linux
probe script rebuilds it and honors `CARGO_TARGET_DIR` directly; no target
symlink is needed. After the final check, clean that exact Cargo target,
remove that task's temporary directory, and verify `du`, `df` and `git status`.
Do not run another build after cleanup.

## Desktop CI caches

Windows release, smoke and OCR installer jobs use `setup-windows-ocr` with
`x64-windows-static-md`. Its files cache includes the hosted vcpkg revision
and runner image version; `vcpkg install` always validates/restores compatible
packages. Model downloads retain their checksums in each workflow.

Rust caches retain compiler, manifest, lockfile and environment hashing and
exclude workspace crates. The pinned rust-cache action ignores `key` when
`shared-key` is set, so compatibility identities are part of `shared-key` itself.
Windows release and smoke share a release-profile
group keyed by the native compatibility identity. OCR installers have their
own groups. Linux Ubuntu 22.04 release, smoke debug and OCR installer groups
are separate, with runner image identity included. Tool-install, preflight
and scraper jobs keep their existing job-specific caches.

The weekly smoke schedule runs on the default branch (`develop`), warming a
cache that releases on `main` can restore. Pull-request caches are scoped to
the PR merge ref and cannot seed a production release. Feature-branch
benchmarks prove restoration within that branch; after integration, confirm
a scheduled or manually dispatched smoke run on `develop` saves the shared
cache. Production timing is confirmed at the next authorized release.

For cold/warm measurements, manually dispatch `ui-smoke` or
`ocr-windows-test` twice on the same revision with the same unique
`cache-namespace`, then change the namespace for an invalidation run. The
namespace prefixes both the native archive identity and the downstream
Windows Rust compatibility key. Omit it for ordinary runs. Do not delete
shared caches or dispatch a production release for benchmarking. The OCR
installer workflow's PR jobs retain their existing branch restriction;
use manual dispatch for these benchmarks.

Record runner image, Rust version, resolved cache keys, archive sizes,
restore/save durations, vcpkg restored-package counts, build time and total
job time. Check cache retention during ordinary CI activity as well as
consecutive warm runs. A GitHub cache hit alone does not establish package
reuse or current-source compilation.

## Measured and rejected

- `arithmetic_side_effects`: roughly 31 distinct sites, dominated by index
  counters and deliberate date arithmetic; no useful finding.
- `as_conversions`: roughly 15 sites, mostly date math and bounded conversions.
- Pedantic and nursery groups: too much unrelated churn for the defects they
  exposed here.
- A repository `rust-toolchain.toml`: `channel = "stable"` made rustup sync on
  every Cargo invocation, harming offline and sandboxed builds. CI already
  selects stable explicitly.
- Nightly, criterion, and rayon: no measured product bottleneck justifies them.
- A Nix/devenv-only workflow: it would not serve Windows contributors or the
  native CI runners equally.

## Findings that justified the policy

- `Duration - elapsed` appeared in four pacing paths. Duration subtraction
  panics whenever a request outlives its interval; checked or saturating
  arithmetic now makes that state harmless.
- Error previews sliced UTF-8 strings at arbitrary byte counts. A non-ASCII WFM
  response could therefore turn an HTTP error into a process crash.
- Several collection indexes were only safe because of nearby loop bounds.
  Those now use checked access or carry a narrow invariant where rewriting the
  hot buffer loop would make it less clear.

The retired source-text gate could only count `unwrap()` and `expect()` tokens.
Clippy sees macro expansion, indexing, string boundaries, explicit panics, and
unchecked time arithmetic, so it guards the failure family rather than two
spellings of it.

## Tauri macro exceptions

- `#[tauri::command]` injects `unreachable!()` into async wrappers. Command
  modules allow that lint file-locally with the macro expansion as the reason.
- `generate_context!()` expands to `process::exit` for invalid build context.
  The builder statement in `main` carries the narrow exception.
