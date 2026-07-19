#!/usr/bin/env python3
"""Semantic JSON comparison for the converter parity gate.

Compares two `market.json` documents *semantically*, not byte-for-byte:
recursively sorted keys, int and float collapsed to a 9-dp float (so 5 and 5.0
compare equal), floats compared within 1e-9, and NaN / +Inf / -Inf on either
side flagged as a discrepancy. Timestamp keys that legitimately differ between
two converter runs (each stamps from its own wall clock / injected `--now`) are
ignored by default.

Single source of truth: `tests/test_convert_parity.py` imports `canonicalize`,
`canonical_diff`, and `TIMESTAMP_KEYS` from here (so the CI parity gate and the
deploy-box shadow share one comparison), and `deploy/run-scrape.sh` shells out
to the CLI to journal shadow parity after each production scrape.

Stdlib only — this runs under the box's minimal venv (requests only).

CLI:
    python3 scripts/semantic_diff.py a.json b.json
      exit 0  parity (ignoring timestamp keys)
      exit 1  differences — prints the true count, then up to 20 paths
      exit 2  usage / read / parse error
"""

import json
import sys

# Timestamps legitimately differ between the two runs (Python stamps
# updated_at / surface_fetched_at from its wall clock, Rust from the injected
# --now / its own clock), so they are filtered out of material differences.
TIMESTAMP_KEYS = ("updated_at", "surface_fetched_at")

# The CLI bounds its printout; the summary line still reports the TRUE count.
MAX_SHOWN = 20


def canonicalize(obj):
    """Recursively sort dict keys and normalize numbers for comparison.
    int and float both collapse to a 9-dp float so 5 and 5.0 compare equal."""
    if isinstance(obj, dict):
        return {k: canonicalize(v) for k, v in sorted(obj.items())}
    if isinstance(obj, list):
        return [canonicalize(x) for x in obj]
    if isinstance(obj, bool):
        return obj
    if isinstance(obj, (int, float)):
        if isinstance(obj, float) and obj != obj:  # NaN
            return "__NaN__"
        if isinstance(obj, float) and obj == float("inf"):
            return "__Infinity__"
        if isinstance(obj, float) and obj == float("-inf"):
            return "__-Infinity__"
        return float(round(obj, 9))
    return obj


def canonical_diff(a, b):
    """Return [(path, a_val, b_val)] for every discrepancy. NaN/Inf on either
    side is a discrepancy; floats compare within 1e-9. A missing key and an
    explicit null both canonicalize to None, so a converter's
    skip-if-none omissions (e.g. ducats=None) don't read as mismatches."""
    discrepancies = []

    def _walk(path, a, b):
        ca = canonicalize(a)
        cb = canonicalize(b)
        if isinstance(ca, str) and ca.startswith("__") and ca != cb:
            discrepancies.append((path, a, b))
            return
        if type(ca) != type(cb):
            discrepancies.append((path, a, b))
            return
        if isinstance(ca, dict):
            keys = set(ca.keys()) | set(cb.keys())
            for k in sorted(keys):
                _walk(f"{path}.{k}", ca.get(k), cb.get(k))
        elif isinstance(ca, list):
            if len(ca) != len(cb):
                discrepancies.append((f"{path}.len", len(a), len(b)))
                return
            for i in range(len(ca)):
                _walk(f"{path}[{i}]", ca[i], cb[i])
        elif isinstance(ca, float):
            if abs(ca - cb) > 1e-9:
                discrepancies.append((path, a, b))
        else:
            if ca != cb:
                discrepancies.append((path, a, b))

    _walk("", a, b)
    return discrepancies


def diff(a, b, ignore_keys=TIMESTAMP_KEYS):
    """Material differences between two JSON docs as human-readable strings,
    with timestamp-only paths filtered out. Empty list means semantic parity.
    Each string is `<path>  a=<json>  b=<json>` (root path renders as
    `<root>`)."""
    out = []
    for path, av, bv in canonical_diff(a, b):
        if any(k in path for k in ignore_keys):
            continue
        out.append(f"{path or '<root>'}  a={json.dumps(av)}  b={json.dumps(bv)}")
    return out


def _main(argv):
    if len(argv) != 3:
        print("usage: semantic_diff.py a.json b.json", file=sys.stderr)
        return 2
    try:
        with open(argv[1], encoding="utf-8") as f:
            a = json.load(f)
        with open(argv[2], encoding="utf-8") as f:
            b = json.load(f)
    except (OSError, ValueError) as e:
        print(f"error: {e}", file=sys.stderr)
        return 2

    diffs = diff(a, b)
    if not diffs:
        return 0

    # Line 1 = the TRUE count (parsed by run-scrape.sh); then up to MAX_SHOWN
    # paths, each indented two spaces so the shell can pick the first cheaply.
    print(f"{len(diffs)} differences (showing first {min(len(diffs), MAX_SHOWN)}):")
    for line in diffs[:MAX_SHOWN]:
        print(f"  {line}")
    return 1


if __name__ == "__main__":
    sys.exit(_main(sys.argv))
