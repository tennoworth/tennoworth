"""Shared scaffolding for the two Rust-parity gates.

The parity tests used to gate on `RUST_BINARY.exists()` alone, which made them
answer the wrong question. A present-but-stale binary ran happily and reported
green: on 2026-08-01 the suite passed five parity tests in 0.08 s against a
binary eight days old and 17 KB different from what the source produced. The
gate exists to catch cross-language drift, so a gate that tests last week's
artifact is the same silent rot the fixture pattern was built to prevent —
just wearing a passing test as a disguise.

So: build every session, and let Cargo decide whether that's a no-op. Cargo's
staleness tracking is the authority on "is this binary current"; reimplementing
it here as an mtime scan would mean hand-maintaining wfm-scrape's dependency
list (market-math, wfm-client, and their manifests), which is exactly the kind
of list that rots. Warm builds cost ~0.1 s; cold ones ~17 s.

CI already builds the binary in its own step before pytest. This fixture makes
that step redundant rather than load-bearing — the gate is now self-contained.
"""

import os
import shutil
import subprocess
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parent.parent
RUST_BINARY = ROOT / "companion" / "target" / "release" / "wfm-scrape"
CARGO_BUILD = "cd companion && cargo build --release -p wfm-scrape"


@pytest.fixture(scope="session")
def rust_binary():
    """Build `wfm-scrape` in release mode and return its path.

    Two failure modes, deliberately handled differently:

    - **No cargo on PATH** — you don't have the toolchain. FAIL under CI (where
      the gate must be real), skip locally. This is the case the old
      `exists()` check was really aiming at.
    - **Build failed** — the Rust source is broken. Always an error, never a
      skip: parity against a binary that won't compile is not a question we can
      answer, and skipping would hide a genuinely broken tree.
    """
    if shutil.which("cargo") is None:
        msg = "cargo not on PATH, cannot build the wfm-scrape parity binary"
        if os.environ.get("CI"):
            pytest.fail(f"{msg} — CI must provide the Rust toolchain")
        pytest.skip(f"{msg} — install Rust, or run: {CARGO_BUILD}")

    result = subprocess.run(
        ["cargo", "build", "--release", "-p", "wfm-scrape"],
        cwd=str(ROOT / "companion"),
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        raise RuntimeError(
            f"`{CARGO_BUILD}` failed (exit {result.returncode}) — the parity gate "
            f"cannot run against a binary that does not compile.\n"
            f"STDOUT:\n{result.stdout}\nSTDERR:\n{result.stderr}"
        )
    if not RUST_BINARY.exists():
        raise RuntimeError(
            f"cargo reported success but {RUST_BINARY} is missing — has the "
            f"binary been renamed or the target dir relocated?"
        )
    return RUST_BINARY
