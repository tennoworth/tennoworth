"""Tests for wfm_demand.py's snapshot/write logic.

We test the pure helpers (build_snapshot, write_snapshot) rather than the
CLI loop — the network-bound `main()` is intentionally out of scope here.

Run from project root:
    PYTHONPATH=. pytest tests/test_wfm_demand.py -v
"""
import csv
import json
import os
import sys
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from wfm_demand import build_snapshot, write_snapshot  # noqa: E402


def _row(slug, **overrides):
    """Synthesize one analyzed row in the exact shape analyze_item() returns."""
    base = {
        "url_name": slug,
        "name": slug.replace("_", " ").title(),
        "tags": [],
        "ducats": None,
        "live_buys": 1,
        "live_sells": 1,
        "buy_sell_ratio": 1.0,
        "top_buy_price": 10,
        "low_sell_price": 12,
        "spread": 2,
        "volume_48h": 5,
        "avg_price_48h": 11.0,
        "median_90d": 11.0,
        "medians_7d": [],
        "donch_top_90d": 0,
        "donch_bot_90d": 0,
        "score": 100.0,
    }
    base.update(overrides)
    return base


# ---- build_snapshot --------------------------------------------------------

def test_snapshot_top_level_shape():
    rows = [_row("axi_k2_relic"), _row("primed_continuity")]
    catalog = {"axi k2 relic": "axi_k2_relic", "primed continuity": "primed_continuity"}
    snap = build_snapshot(rows, platform="pc", catalog=catalog, final=True)
    assert set(snap.keys()) == {
        "updated_at", "platform", "item_count", "catalog_count",
        "catalog", "items", "partial",
    }
    assert snap["platform"] == "pc"
    assert snap["item_count"] == 2
    assert snap["catalog_count"] == 2
    assert snap["partial"] is False  # final=True ⇒ partial=False


def test_snapshot_partial_flag_flips_with_final():
    rows = [_row("test")]
    assert build_snapshot(rows, platform="pc", catalog={}, final=False)["partial"] is True
    assert build_snapshot(rows, platform="pc", catalog={}, final=True)["partial"] is False


def test_snapshot_item_shape_uses_short_keys():
    """The web UI relies on short keys (avg/low_sell/...) — guard against drift."""
    rows = [_row("zephyr_prime_set",
                 avg_price_48h=42.5, low_sell_price=40, top_buy_price=44,
                 volume_48h=99, buy_sell_ratio=0.5, live_buys=10, live_sells=20,
                 tags=["prime", "warframe"], ducats=15,
                 median_90d=44.0, medians_7d=[42, 43, 44, 45, 44, 44, 44],
                 donch_top_90d=60, donch_bot_90d=30)]
    snap = build_snapshot(rows, platform="pc", catalog={}, final=True)
    entry = snap["items"]["zephyr_prime_set"]
    assert entry == {
        "avg": 42.5, "low_sell": 40, "top_buy": 44,
        "vol": 99, "ratio": 0.5, "buys": 10, "sells": 20,
        "tags": ["prime", "warframe"],
        "ducats": 15,
        "median_90d": 44.0,
        "medians_7d": [42, 43, 44, 45, 44, 44, 44],
        "donch_top_90d": 60,
        "donch_bot_90d": 30,
    }


def test_snapshot_item_shape_includes_default_extended_fields_when_absent():
    """Rows from an older analyze_item (no extended fields) still snapshot
    cleanly — keys are present with safe defaults so the browser never
    sees `undefined.tags` etc."""
    bare = {
        "url_name": "old_row",
        "name": "Old Row",
        "live_buys": 0, "live_sells": 0, "buy_sell_ratio": 0,
        "top_buy_price": 0, "low_sell_price": 0, "spread": 0,
        "volume_48h": 0, "avg_price_48h": 0.0, "score": 0.0,
    }
    snap = build_snapshot([bare], platform="pc", catalog={}, final=True)
    entry = snap["items"]["old_row"]
    assert entry["tags"] == []
    assert entry["ducats"] is None
    assert entry["median_90d"] == 0
    assert entry["medians_7d"] == []
    assert entry["donch_top_90d"] == 0
    assert entry["donch_bot_90d"] == 0


def test_snapshot_catalog_preserved_verbatim():
    catalog = {"foo": "foo_set", "bar baz": "bar_baz"}
    snap = build_snapshot([_row("foo_set")], platform="pc", catalog=catalog, final=True)
    assert snap["catalog"] is catalog  # we don't copy; share the reference
    assert snap["catalog_count"] == 2


def test_snapshot_updated_at_is_utc_iso_z():
    snap = build_snapshot([_row("x")], platform="pc", catalog={}, final=True)
    ts = snap["updated_at"]
    assert ts.endswith("Z")
    assert "T" in ts
    # Sanity: parseable.
    from datetime import datetime
    datetime.strptime(ts, "%Y-%m-%dT%H:%M:%SZ")


# ---- write_snapshot --------------------------------------------------------

def test_write_emits_both_csv_and_json(tmp_path):
    csv_path = tmp_path / "out.csv"
    json_path = tmp_path / "out.json"
    rows = [_row("foo_set", score=200.0), _row("bar_set", score=100.0)]
    write_snapshot(rows, csv_path=str(csv_path), json_path=str(json_path),
                   platform="pc", catalog={"foo": "foo_set", "bar": "bar_set"},
                   final=True)
    assert csv_path.exists()
    assert json_path.exists()
    snap = json.loads(json_path.read_text())
    # Sorted by score desc, so foo_set comes first.
    assert list(snap["items"].keys())[0] == "foo_set"


def test_write_no_json_when_path_omitted(tmp_path):
    csv_path = tmp_path / "out.csv"
    json_path = tmp_path / "out.json"
    write_snapshot([_row("x")], csv_path=str(csv_path), json_path=None,
                   platform="pc", catalog={}, final=True)
    assert csv_path.exists()
    assert not json_path.exists()


def test_write_skips_when_results_empty(tmp_path):
    csv_path = tmp_path / "out.csv"
    json_path = tmp_path / "out.json"
    write_snapshot([], csv_path=str(csv_path), json_path=str(json_path),
                   platform="pc", catalog={}, final=True)
    assert not csv_path.exists()
    assert not json_path.exists()


def test_write_is_atomic_no_tmp_left_behind(tmp_path):
    csv_path = tmp_path / "out.csv"
    json_path = tmp_path / "out.json"
    write_snapshot([_row("x")], csv_path=str(csv_path), json_path=str(json_path),
                   platform="pc", catalog={}, final=True)
    # No `.tmp` artifacts should remain on a clean write.
    leftovers = list(tmp_path.glob("*.tmp"))
    assert leftovers == []


def test_write_overwrites_existing_file(tmp_path):
    csv_path = tmp_path / "out.csv"
    json_path = tmp_path / "out.json"
    csv_path.write_text("OLD")
    json_path.write_text('{"old": true}')
    write_snapshot([_row("new_item")], csv_path=str(csv_path), json_path=str(json_path),
                   platform="pc", catalog={}, final=True)
    assert csv_path.read_text() != "OLD"
    assert json.loads(json_path.read_text())["items"].get("new_item") is not None


def test_write_csv_columns_match_row_keys(tmp_path):
    csv_path = tmp_path / "out.csv"
    rows = [_row("x")]
    write_snapshot(rows, csv_path=str(csv_path), json_path=None,
                   platform="pc", catalog={}, final=True)
    with open(csv_path, newline="") as f:
        reader = csv.DictReader(f)
        header = reader.fieldnames
        first = next(reader)
    # Header == keys of an analyzed row, in the same order.
    assert header == list(rows[0].keys())
    assert first["url_name"] == "x"


def test_write_partial_then_final_keeps_both_files_consistent(tmp_path):
    """Simulating the checkpoint loop: two flushes (partial, then final).
    Both must produce a fully-readable file at every moment."""
    csv_path = tmp_path / "out.csv"
    json_path = tmp_path / "out.json"
    write_snapshot([_row("a")], csv_path=str(csv_path), json_path=str(json_path),
                   platform="pc", catalog={}, final=False)
    snap1 = json.loads(json_path.read_text())
    assert snap1["partial"] is True
    write_snapshot([_row("a"), _row("b")], csv_path=str(csv_path), json_path=str(json_path),
                   platform="pc", catalog={}, final=True)
    snap2 = json.loads(json_path.read_text())
    assert snap2["partial"] is False
    assert snap2["item_count"] == 2
