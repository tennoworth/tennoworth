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
are separate, with runner image identity included. Tool-install, preflight,
scraper and audit jobs keep their existing job-specific caches.

The default branch (`develop`) is the only scope a pull request can restore from
beyond its own merge ref, so it must be seeded by a non-PR run. Two triggers do
that: the weekly smoke/audit crons, and a `push` trigger on `develop` filtered to
the files that rotate a cached key (Cargo manifests and lockfiles,
`frontend/bun.lock`, and the setup actions themselves). The weekly cron alone
proved insufficient - see "Cache scope fix" below. A cache a pull request writes
goes to its merge ref and cannot seed anything else, which is also why pull
requests are now restore-only. Feature-branch benchmarks prove restoration
within that branch; after integration, confirm a scheduled, pushed or manually
dispatched run on `develop` saves the shared cache. Production timing is
confirmed at the next authorized release.

For cold/warm measurements, manually dispatch `ui-smoke` or
`ocr-windows-test` twice on the same revision with the same unique
`cache-namespace`, then change the namespace for an invalidation run. The
namespace prefixes both the native archive identity and the downstream
Windows Rust compatibility key. Omit it for ordinary runs. Do not delete
shared caches or dispatch a production release for benchmarking. The OCR
installer workflow no longer has a `pull_request` trigger - its job-level
branch guard had gone dead once that branch merged - so use manual dispatch for
these benchmarks.

Record runner image, Rust version, resolved cache keys, archive sizes,
restore/save durations, vcpkg restored-package counts, build time and total
job time. Check cache retention during ordinary CI activity as well as
consecutive warm runs. A GitHub cache hit alone does not establish package
reuse or current-source compilation.

### Audit cache experiment (September 9)

Three warm runs with identical application source compared shared test/Clippy
caches against the prior job-specific caches. Combined job seconds were:

| Sample | Separate caches | Shared cache |
|---|---:|---:|
| 1 | 248 | 317 |
| 2 | 251 | 318 |
| 3 | 214 | 189 |
| Mean | 238 | 275 |

Sharing regressed mean combined runner time by 15.6%, exceeding the 10%
rollback threshold. Audit jobs therefore retain their separate keys. In the
first two shared samples, Clippy restored the test job's roughly 1 GB archive
and still compiled its own outputs. Hosted runner image versions were rolling
between `20260831.293.1` and `20260907.300.1`; the experiment kept those keys
separate. The third sample was faster, so the mean does not imply every run
regresses.

Controls: [audit run 34290649818, attempts 2–4](https://github.com/tennoworth/tennoworth/actions/runs/34290649818).
Shared samples: [1](https://github.com/tennoworth/tennoworth/actions/runs/34294765431),
[2](https://github.com/tennoworth/tennoworth/actions/runs/34295140317),
[3](https://github.com/tennoworth/tennoworth/actions/runs/34295564135).

### Cache scope fix (September 13)

Every pull request was compiling cold. `gh cache list` showed no entries on
`refs/heads/develop` at all, while 9.46 GiB of the repository's 11.29 GiB sat on
ephemeral `refs/pull/*/merge` refs that no other run may read. The logs named the
mechanism directly: run 34728226930 logged `No cache found.` in `cargo-test`,
`cargo-clippy`, `cargo-shear` and `cargo-audit`, and run 34603856882's Windows
probe logged `Cache not found for input keys:
vcpkg-Windows-x64-windows-static-md-...-20260907.229.1`, then `Restored 0
package(s)` and `Total install time: 18 min`. The key matched exactly and the
entry existed - on refs a pull request is not allowed to restore from.

The cause was cadence, not configuration. `rust-cache` hashes the Cargo
manifests into its key, `rust/Cargo.lock` changed on September 8, 9 and 11, and
the only writer to develop's scope was a Monday cron. Each dependency merge
rotated every Rust key and left the rest of the week cold: `cargo-test` at 5m54s
against a 2m49s warm run, `cargo-audit` at 3m11s against 0m10s, and the Windows
probe at a 31m31s median against a 9m30s best case. The cron's cache had not so
much been evicted as outrun.

Three changes follow. `setup-rust` now passes `save-if` so pull requests restore
without saving, which stops merge-ref archives from consuming the 10 GB
repository budget and evicting the seeds. `setup-windows-ocr` splits
`actions/cache/restore` from `actions/cache/save` for the same reason, and both
save steps carry `continue-on-error` so losing a save race cannot fail a build.
`audit` and `ui-smoke` now also trigger on pushes to `develop`, restricted to the
files that rotate a cached key, so a new key is seeded within minutes of the
merge that created it rather than the following Monday.

Sharing audit's Rust caches was deliberately not revisited; the September 9
experiment below measured that separately, and this was a different fault.
Publishing the vcpkg archive as a release asset was considered and dropped: the
key already matched, so only the ref was wrong, and the scoping fix addresses
that without new machinery.

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
