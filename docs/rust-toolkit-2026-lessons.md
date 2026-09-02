# 2026 Rust toolkit decisions

This note records which recommendations from
[My 2026 Rust Toolkit](https://www.namtao.com/rust-toolkit-2026/) were adopted
and which were measured and rejected.

## Adopted

| Lesson | Where it landed |
|---|---|
| Deny-panics Clippy policy | `companion/Cargo.toml` `[workspace.lints.clippy]`, `companion/clippy.toml`, and the `cargo-clippy` job feeding `audit-gate` |
| Local, reasoned lint exceptions | Sites unreachable by construction carry `#[allow(<lint>, reason = "...")]` beside the invariant |
| `cargo shear` | Local dead-dependency sweep; the Rust counterpart to `knip` |
| `cargo nextest` | Faster local test runner; `cargo test` remains the CI command |
| `cargo info` before `cargo add` | Dependency review checks license, Rust floor, and feature surface before adoption |
| Bacon | Optional local Clippy loop; fix the first compiler diagnostic first |

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
