"""Semantic parity gate: run the Python scraper (`wfm_demand.py`) and the Rust
`wfm-scrape scrape` subcommand on the SAME frozen fixtures, then compare their
output CSVs semantically — matched on `url_name`, every column compared by
parsed value with a small float tolerance. Column order and float formatting
may differ textually; the parsed values must not.

Both sides read the SAME URL→response map: the Python side via a patched
`requests.Session.get` with `time.sleep` stubbed out, the Rust side via
`wfm-scrape scrape --fixtures-dir` (which serves that same map and never sleeps).

FIXTURE RESPONSE FORMAT (kept byte-identical on both sides, see
`_fake_get_factory` here and `FixtureScrapeHttp` in http.rs):
  - a bare JSON body (object)            → HTTP 200 with that body,
  - `{"status": <int>, "body": <json>}`  → that status (429 → rate-limited,
    other non-2xx → error), the given body on 2xx,
  - a JSON ARRAY                         → a scripted SEQUENCE, one element
    consumed per GET to the same URL (retry scripting: `[429, 429, 200]`),
    sticky-last once exhausted. WFM bodies are always envelope objects, never
    bare arrays, so a top-level array is unambiguously a sequence.

Every artifact is written to a pytest tmp dir, so the committed fixtures never
gain stray output files.

Missing Rust binary: FAIL under CI (the gate must be real there), skip locally
with the build command — the same policy as test_convert_parity.py.

Run: pytest tests/test_scrape_parity.py -v
"""

import csv
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

import requests  # noqa: E402
import wfm_demand  # noqa: E402

FIXTURES = HERE / "fixtures" / "scrape"
RESPONSES_PATH = FIXTURES / "fixture_responses.json"
RUST_BINARY = ROOT / "companion" / "target" / "release" / "wfm-scrape"
NOW_ISO = "2026-07-01T12:00:00Z"
CARGO_BUILD = "cd companion && cargo build --release -p wfm-scrape"

# Column classification for the semantic compare. `url_name` is the join key;
# `name` is exact text; `tags` and `medians_7d` are list reprs; everything else
# is numeric (ducats may be blank for a null-ducat item).
KEY_COL = "url_name"
TEXT_COLS = {"name"}
STR_LIST_COLS = {"tags"}
NUM_LIST_COLS = {"medians_7d"}
TOL = 1e-6


# ---- shared scripted-response fake (mirrors http.rs FixtureScrapeHttp) --

def _require_rust_binary():
    if not RUST_BINARY.exists():
        msg = f"wfm-scrape release binary not found at {RUST_BINARY}"
        if os.environ.get("CI"):
            pytest.fail(f"{msg} — CI must build it first: {CARGO_BUILD}")
        pytest.skip(f"{msg} — build it locally: {CARGO_BUILD}")


def _fake_get_factory(responses):
    """Return a `Session.get` replacement that serves scripted responses from a
    URL→spec map, byte-identically to the Rust FixtureScrapeHttp. Per-URL call
    cursors advance a scripted sequence one response per call."""
    cursors = {}

    class FakeResponse:
        def __init__(self, status, body):
            self.status_code = status
            self._body = body

        def json(self):
            return self._body

        def raise_for_status(self):
            # requests raises for >= 400; 429 is handled before this is called.
            if self.status_code >= 400:
                raise requests.HTTPError(f"HTTP {self.status_code}")

    def interpret_one(v):
        if isinstance(v, dict) and "status" in v:
            return v["status"], v.get("body")
        return 200, v

    def response_at(value, i):
        if isinstance(value, list):
            if not value:
                return 200, None
            return interpret_one(value[min(i, len(value) - 1)])
        return interpret_one(value)

    def fake_get(self, url, timeout=30, **kwargs):
        key = url if url in responses else url.rstrip("/")
        value = responses.get(key)
        if value is None:
            raise requests.RequestException(f"Unexpected URL: {url}")
        i = cursors.get(key, 0)
        cursors[key] = i + 1
        status, body = response_at(value, i)
        return FakeResponse(status, body)

    return fake_get


# ---- Python scraper runner (patched Session.get + time.sleep) ----------

def run_python_scraper(work_dir, args_extra, out_name="wfm_results_py.csv"):
    """Run wfm_demand.main() against `work_dir/fixture_responses.json` with the
    network mocked and sleeps stubbed. Returns the CSV it wrote."""
    responses = json.loads((work_dir / "fixture_responses.json").read_text())
    out = work_dir / out_name
    fake_get = _fake_get_factory(responses)

    argv = ["wfm_demand.py"] + list(args_extra) + ["--out", str(out)]
    with mock.patch.object(requests.Session, "get", autospec=True, side_effect=fake_get), \
         mock.patch("time.sleep", lambda *a, **k: None), \
         mock.patch.object(sys, "argv", argv):
        wfm_demand.main()

    return out


# ---- Rust scraper runner (subprocess) ---------------------------------

def run_rust_scraper(work_dir, args_extra, out_name="wfm_results_rs.csv"):
    """Run `wfm-scrape scrape --fixtures-dir <work_dir>`. Missing binary: FAIL
    under CI, skip locally — same policy as convert parity."""
    _require_rust_binary()

    out = work_dir / out_name
    cmd = [str(RUST_BINARY), "scrape", "--fixtures-dir", str(work_dir)] \
        + list(args_extra) + ["--out", str(out), "--now", NOW_ISO]
    result = subprocess.run(cmd, capture_output=True, text=True, cwd=str(ROOT))
    if result.returncode != 0:
        raise RuntimeError(
            f"Rust scraper failed (exit {result.returncode}):\n"
            f"STDOUT:\n{result.stdout}\nSTDERR:\n{result.stderr}"
        )
    return out


# ---- semantic CSV compare ---------------------------------------------

def parse_csv(path):
    with open(path, newline="", encoding="utf-8") as f:
        reader = csv.DictReader(f)
        rows = {r[KEY_COL]: r for r in reader}
        header = reader.fieldnames
    return rows, header


def _num(s):
    return None if s is None or s == "" else float(s)


def _num_list(s):
    s = (s or "").strip()
    if s in ("", "[]"):
        return []
    return [float(p) for p in s[1:-1].split(",") if p.strip() != ""]


def _str_list(s):
    s = (s or "").strip()
    if s in ("", "[]"):
        return []
    return [p.strip().strip("'\"") for p in s[1:-1].split(",")]


def compare_rows(py_rows, rs_rows):
    diffs = []
    py_keys, rs_keys = set(py_rows), set(rs_rows)
    if py_keys != rs_keys:
        diffs.append(("<row set>", sorted(py_keys), sorted(rs_keys)))
        return diffs

    columns = [c for c in py_rows[next(iter(py_rows))].keys()] if py_rows else []
    for slug in sorted(py_keys):
        pr, rr = py_rows[slug], rs_rows[slug]
        for col in columns:
            pv, rv = pr.get(col, ""), rr.get(col, "")
            if col == KEY_COL or col in TEXT_COLS:
                if pv != rv:
                    diffs.append((f"{slug}.{col}", pv, rv))
            elif col in STR_LIST_COLS:
                if _str_list(pv) != _str_list(rv):
                    diffs.append((f"{slug}.{col}", pv, rv))
            elif col in NUM_LIST_COLS:
                a, b = _num_list(pv), _num_list(rv)
                if len(a) != len(b) or any(abs(x - y) > TOL for x, y in zip(a, b)):
                    diffs.append((f"{slug}.{col}", pv, rv))
            else:  # numeric
                a, b = _num(pv), _num(rv)
                if a is None or b is None:
                    if a != b:
                        diffs.append((f"{slug}.{col}", pv, rv))
                elif abs(a - b) > TOL:
                    diffs.append((f"{slug}.{col}", pv, rv))
    return diffs


def _assert_no_diffs(py_rows, rs_rows):
    diffs = compare_rows(py_rows, rs_rows)
    if diffs:
        for path, pv, rv in diffs:
            print(f"  MISMATCH @ {path}:  Python={pv!r}  Rust={rv!r}")
        pytest.fail(f"{len(diffs)} CSV cell discrepancies between the scrapers")


# ---- tests ------------------------------------------------------------

BASE_ARGS = ["--filter", "", "--exclude", "", "--min-volume", "1"]


def test_scrape_parity(tmp_path):
    """The Python and Rust scrapers on identical fixtures must agree on every
    parsed CSV cell — including the retry-recovery and missing-90days cases.
    `stats_exhaust` (stats 429 x3) is skipped by BOTH, so it appears in neither."""
    work_dir = tmp_path / "scrape"
    work_dir.mkdir()
    shutil.copyfile(RESPONSES_PATH, work_dir / "fixture_responses.json")

    py_csv = run_python_scraper(work_dir, BASE_ARGS)
    py_rows, py_header = parse_csv(py_csv)

    # Structural canaries: every surviving fixture item clears min-volume=1.
    # retry_recover survives (orders 429->429->200); missing_ninetydays survives
    # off its 48h window; stats_exhaust is dropped (stats 429 exhausts retries).
    expected = {
        "volt_prime_barrel", "goopolla", "primed_continuity", "lith_v1_relic",
        "axi_a1_relic", "nova_prime_blueprint", "ivara_prime_set",
        "retry_recover", "missing_ninetydays",
    }
    assert set(py_rows) == expected, f"Python kept {set(py_rows)}, expected {expected}"
    assert "stats_exhaust" not in py_rows, "stats_exhaust must be skipped (stats 429 x3)"

    # May skip (local) or fail (CI) if the release binary isn't built.
    rs_csv = run_rust_scraper(work_dir, BASE_ARGS)
    rs_rows, rs_header = parse_csv(rs_csv)

    # Both must carry the same 19-column header (order-insensitive set check).
    assert set(py_header) == set(rs_header), (
        f"header mismatch:\n  py={py_header}\n  rs={rs_header}"
    )

    _assert_no_diffs(py_rows, rs_rows)
    print(f"  OK — {len(py_rows)} items agree across all columns")


def test_scrape_parity_limit(tmp_path):
    """--limit truncates AFTER filter/exclude, identically on both sides (Python
    slices `items[:limit]`; Rust `items.truncate(limit)` — same first-N)."""
    work_dir = tmp_path / "limit"
    work_dir.mkdir()
    shutil.copyfile(RESPONSES_PATH, work_dir / "fixture_responses.json")
    args = BASE_ARGS + ["--limit", "4"]

    py_rows, _ = parse_csv(run_python_scraper(work_dir, args))
    rs_rows, _ = parse_csv(run_rust_scraper(work_dir, args))

    # The first four master-list items after the (empty) filter/exclude; all
    # clear the volume gate, so exactly these four survive on both sides.
    first_four = {"volt_prime_barrel", "goopolla", "primed_continuity", "lith_v1_relic"}
    assert set(py_rows) == first_four, f"limit truncation drifted: {set(py_rows)}"
    _assert_no_diffs(py_rows, rs_rows)
    print(f"  OK — --limit 4 truncated identically to {sorted(py_rows)}")


def test_scrape_parity_checkpoint_boundary(tmp_path):
    """>100 synthetic items force a checkpoint. Both CSVs must agree; the Rust
    run writes its checkpoint to `<out>.partial` and replaces `--out` exactly
    once, at completion (the deliberate divergence from Python's in-place
    checkpoint)."""
    work_dir = tmp_path / "ckpt"
    work_dir.mkdir()

    n = 105
    catalog = []
    responses = {}
    for i in range(n):
        slug = f"synthetic_item_{i:04d}"
        catalog.append({"slug": slug, "i18n": {"en": {"name": slug}}, "tags": [], "ducats": None})
        responses[f"https://api.warframe.market/v2/orders/item/{slug}"] = {"data": [
            {"type": "buy", "platinum": 20, "visible": True, "user": {"status": "ingame"}},
            {"type": "sell", "platinum": 25, "visible": True, "user": {"status": "ingame"}},
        ]}
        responses[f"https://api.warframe.market/v1/items/{slug}/statistics"] = {"payload": {"statistics_closed": {
            "48hours": [{"median": 25, "max_price": 30, "volume": 5, "avg_price": 25.0}],
            "90days": [
                {"median": 24, "max_price": 29, "volume": 4, "avg_price": 24.0},
                {"median": 25, "max_price": 30, "volume": 5, "avg_price": 25.0},
            ],
        }}}
    responses["https://api.warframe.market/v2/items"] = {"data": catalog}
    (work_dir / "fixture_responses.json").write_text(json.dumps(responses))

    py_rows, _ = parse_csv(run_python_scraper(work_dir, BASE_ARGS))
    assert len(py_rows) == n, f"Python kept {len(py_rows)}, expected {n}"

    rs_out = run_rust_scraper(work_dir, BASE_ARGS)
    rs_rows, _ = parse_csv(rs_out)
    assert len(rs_rows) == n, f"final --out holds {len(rs_rows)} rows, expected {n}"

    # New semantics: a checkpoint fired at item 100 and landed on <out>.partial,
    # never on --out. Its presence proves the checkpoint boundary was crossed.
    partial = rs_out.parent / (rs_out.name + ".partial")
    assert partial.exists(), "a checkpoint must write <out>.partial"
    with open(partial, newline="", encoding="utf-8") as f:
        partial_count = sum(1 for _ in csv.reader(f)) - 1  # minus header
    assert partial_count == 100, f"checkpoint captured the first 100 items, got {partial_count}"

    _assert_no_diffs(py_rows, rs_rows)
    print(f"  OK — {n} items agree; checkpoint fired to .partial ({partial_count} rows)")


def test_rounding_parity():
    """Feed the classic tie / binary-half cases through BOTH languages' actual
    rounding paths — Python's `round(x, n)` and the release binary's `round_dp`
    via the hidden `round-check` subcommand — and assert bit-for-bit agreement.
    This shells the REAL binary rather than trusting a Rust-side unit test."""
    _require_rust_binary()

    vectors = [
        (2.675, 2), (1.005, 2), (2.25, 1), (2.35, 1), (0.125, 2),
        # adjacent binary-half cases: exact ties and near-ties on either side.
        (0.375, 2), (0.135, 2), (0.145, 2), (2.5, 0), (3.5, 0),
        (0.5, 0), (1.5, 0), (-2.675, 2), (2.665, 2), (1.255, 2),
    ]
    stdin = "".join(f"{x},{n}\n" for x, n in vectors)
    result = subprocess.run(
        [str(RUST_BINARY), "round-check"],
        input=stdin, capture_output=True, text=True, cwd=str(ROOT),
    )
    assert result.returncode == 0, f"round-check failed: {result.stderr}"
    rust_lines = result.stdout.split()
    assert len(rust_lines) == len(vectors), (
        f"expected {len(vectors)} result lines, got {rust_lines}"
    )

    mismatches = []
    for (x, n), rline in zip(vectors, rust_lines):
        py = round(x, n)
        rs = float(rline)
        if rs != py:
            mismatches.append((x, n, py, rs))
    assert not mismatches, f"rounding divergences (x, n, python, rust): {mismatches}"
    print(f"  OK — {len(vectors)} rounding vectors agree Python vs Rust")


if __name__ == "__main__":
    pytest.main([__file__, "-v"])
