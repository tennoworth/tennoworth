"""Semantic parity gate: run the Python and Rust converters on the SAME
frozen fixtures, then compare their `market.json` outputs semantically —
canonicalized (recursively sorted keys), fixed/ignored timestamps, small
float tolerance. NOT a byte-diff.

Both converters read only `tests/fixtures/convert/`: `fixture_responses.json`
stands in for live HTTP on both sides (the Python side via a patched
`requests.get`, the Rust side via `wfm-scrape build --fixtures-dir`). Every
generated artifact is written to a pytest tmp dir, so the committed fixtures
never gain stray output files.

Run: pytest tests/test_convert_parity.py -v
"""

import json
import os
import shutil
import subprocess
import sys
from pathlib import Path
from unittest import mock

import pytest

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
sys.path.insert(0, str(ROOT))

# Semantic comparison lives in scripts/semantic_diff.py — the single source of
# truth shared with the deploy-box converter shadow (deploy/run-scrape.sh).
# Importable here after the sys.path insert above.
from scripts.semantic_diff import canonical_diff, TIMESTAMP_KEYS  # noqa: E402

FIXTURES = HERE / "fixtures" / "convert"
RUST_BINARY = ROOT / "companion" / "target" / "release" / "wfm-scrape"
NOW_ISO = "2026-07-01T12:00:00Z"
CARGO_BUILD = "cd companion && cargo build --release -p wfm-scrape"

# Inputs both converters consume; copied into a tmp work dir per run so the
# Rust binary (which writes market.json next to its --fixtures-dir inputs) and
# the Python run never touch the committed fixtures.
FIXTURE_INPUTS = ("fixture_responses.json", "wfm_results.csv", "prior-market.json")

# canonicalize / canonical_diff / TIMESTAMP_KEYS now live in
# scripts/semantic_diff.py (imported above). Timestamps legitimately differ
# between the two runs — Python stamps updated_at / surface_fetched_at from its
# (module-level, unpatched) wall clock, Rust from the injected --now — so the
# test filters TIMESTAMP_KEYS paths out of the material diff below.


# ---- Python converter runner (patched requests + datetime) ------------

def run_python_converter(work_dir, now_iso=NOW_ISO):
    """Run csv_to_market_json.py against the fixtures with requests and
    datetime patched. Outputs land in `work_dir`."""
    from datetime import datetime

    responses = json.loads((FIXTURES / "fixture_responses.json").read_text())
    patch_val = datetime.fromisoformat(now_iso.replace("Z", "+00:00"))

    class FixedDatetime(datetime):
        @classmethod
        def now(cls, tz=None):
            return patch_val

    out = work_dir / "market-py.json"
    catalog_out = work_dir / "wfstat-catalog-py.json"
    csv_in = work_dir / "wfm_results.csv"

    # Seed the prior so the Python run reconciles against the SAME prior the
    # Rust run reads from prior-market.json — the converter reads its prior
    # from JSON_OUT. Inert while every fixture endpoint returns complete data
    # (reconcile then returns fresh), but keeps the two sides symmetric if a
    # fixture is ever extended to exercise the preserve/merge path.
    shutil.copyfile(FIXTURES / "prior-market.json", out)

    def fake_get(url, timeout=30, headers=None):
        class FakeResponse:
            def __init__(self, status_code, body):
                self.status_code = status_code
                self._body = body

            def json(self):
                return self._body

            def raise_for_status(self):
                if self.status_code >= 400:
                    raise Exception(f"HTTP {self.status_code}")

            @property
            def text(self):
                if isinstance(self._body, (dict, list)):
                    return json.dumps(self._body)
                return str(self._body)

        body = responses.get(url, responses.get(url.rstrip("/")))
        if body is not None:
            return FakeResponse(200, body)
        raise Exception(f"Unexpected URL: {url}")

    # datetime.datetime is patched module-wide. csv_to_market_json binds
    # `datetime` at import time (already cached by test_wfm_demand's import),
    # so its module-level now() may read the real clock — but that only feeds
    # updated_at / surface_fetched_at, which the diff ignores. The
    # vaulting-soon computation lives in fetch_vault_status, which re-imports
    # datetime INSIDE the function under this patch, so vault_status (compared
    # content) is deterministic against now_iso.
    with mock.patch("datetime.datetime", FixedDatetime), \
         mock.patch("requests.get", side_effect=fake_get):
        import scripts.csv_to_market_json as converter

        with mock.patch.object(converter, "CSV_IN", csv_in), \
             mock.patch.object(converter, "JSON_OUT", out), \
             mock.patch.object(converter, "CATALOG_OUT", catalog_out):
            converter.main()

    return json.loads(out.read_text())


# ---- Rust converter runner (subprocess) -------------------------------

def run_rust_converter(work_dir, now_iso=NOW_ISO):
    """Run `wfm-scrape build --fixtures-dir <work_dir> --now`. Missing binary:
    FAIL under CI (the gate must be real there), skip locally with the build
    command."""
    if not RUST_BINARY.exists():
        msg = f"wfm-scrape release binary not found at {RUST_BINARY}"
        if os.environ.get("CI"):
            pytest.fail(f"{msg} — CI must build it first: {CARGO_BUILD}")
        pytest.skip(f"{msg} — build it locally: {CARGO_BUILD}")

    cmd = [str(RUST_BINARY), "build", "--fixtures-dir", str(work_dir), "--now", now_iso]
    result = subprocess.run(cmd, capture_output=True, text=True, cwd=str(ROOT))
    if result.returncode != 0:
        raise RuntimeError(
            f"Rust converter failed (exit {result.returncode}):\n"
            f"STDOUT:\n{result.stdout}\nSTDERR:\n{result.stderr}"
        )

    return json.loads((work_dir / "market.json").read_text())


# ---- semantic canaries -----------------------------------------------

def check_semantic_canaries(snapshot, name):
    """Assert a converter output is structurally sane before diffing, so a
    genuinely broken side fails with a specific message instead of a wall of
    diffs."""
    items = snapshot.get("items", {})
    catalog = snapshot.get("catalog", {})

    required_keys = [
        "updated_at", "platform", "item_count", "catalog_count",
        "source", "catalog", "items", "path_to_info", "set_to_parts",
        "relic_rewards", "vault_status", "baro", "surface_fetched_at",
    ]
    missing = [k for k in required_keys if k not in snapshot]
    assert not missing, f"[{name}] missing keys: {missing}"

    if items:
        priced = sum(1 for it in items.values()
                     if it.get("low_sell", 0) > 0
                     or it.get("avg", 0) > 0
                     or it.get("median_90d", 0) > 0)
        assert priced / len(items) >= 0.9, \
            f"[{name}] only {priced / len(items):.1%} items have nonzero price"

        volumed = sum(1 for it in items.values() if it.get("vol", 0) > 0)
        assert volumed / len(items) >= 0.9, \
            f"[{name}] only {volumed / len(items):.1%} items have nonzero vol"

    assert len(catalog) > 0, f"[{name}] empty catalog"
    assert "primed_continuity" in items, f"[{name}] missing primed_continuity"


# ---- test -------------------------------------------------------------

def test_convert_parity(tmp_path):
    """Both converters on the same frozen inputs must agree on market.json
    beyond the intentionally-diverging timestamp fields."""
    work_dir = tmp_path / "convert"
    work_dir.mkdir()
    for name in FIXTURE_INPUTS:
        shutil.copyfile(FIXTURES / name, work_dir / name)

    py = run_python_converter(work_dir)
    check_semantic_canaries(py, "Python")

    # May skip (local) or fail (CI) if the release binary isn't built.
    rs = run_rust_converter(work_dir)
    check_semantic_canaries(rs, "Rust")

    diffs = canonical_diff(py, rs)
    material = [
        (path, pv, rv) for (path, pv, rv) in diffs
        if not any(k in path for k in TIMESTAMP_KEYS)
    ]

    if material:
        for path, py_val, rs_val in material:
            print(f"  MISMATCH @ {path}:")
            print(f"    Python: {json.dumps(py_val)}")
            print(f"    Rust:   {json.dumps(rs_val)}")
        pytest.fail(f"{len(material)} discrepancies beyond timestamps")

    print(f"  OK — {len(diffs)} total diffs, all within {TIMESTAMP_KEYS}")


if __name__ == "__main__":
    pytest.main([__file__, "-v"])
