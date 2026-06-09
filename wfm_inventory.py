#!/usr/bin/env python3
"""
wfm_inventory.py
----------------
Reads a Warframe inventory.json (produced by the `companion` CLI) and tells
you which of your owned items have real market demand on warframe.market.

Debug-only — the browser app at prototype/ is the canonical surface.

Two modes:
  default  — joins against a local wfm_results.csv (from wfm_demand.py)
  --live   — skips the CSV; fetches live order+stats data from WFM for only
             the items you actually own. Politer to WFM (few hundred calls vs
             thousands) and gives you fresher numbers.

Usage:
    pip install requests
    python wfm_inventory.py                  # CSV mode
    python wfm_inventory.py --live           # live, owned-items-only
    python wfm_inventory.py --live --top 50 --min-price 10
"""

import argparse
import csv
import json
import re
import sys
import time
from collections import defaultdict
from datetime import datetime
from pathlib import Path

import requests

WFM_API = "https://api.warframe.market"
WFM_ITEMS_URL = f"{WFM_API}/v2/items"
WFSTAT_ITEMS_URL = "https://api.warframestat.us/items"
REQUEST_DELAY = 0.34  # ~3 req/sec — WFM's documented ceiling

# Inventory keys that contain things you might actually sell.
# (Consumables / resources are tradeable in theory but rarely worth listing.)
TRADEABLE_CATEGORIES = (
    "MiscItems",     # prime parts — the main thing people trade
    "Recipes",       # blueprints, including prime set BPs
    "RawUpgrades",   # unranked mods (rare/uncommon drops live here)
    "Suits", "LongGuns", "Pistols", "Melee",
    "SpaceGuns", "SpaceMelee", "Sentinels", "SentinelWeapons",
)


def build_name_maps(session):
    """uniqueName -> display name (warframestat.us), display name -> wfm slug."""
    print("Loading warframestat.us items (for path -> name resolution)...")
    wfstat = session.get(WFSTAT_ITEMS_URL, timeout=60).json()
    unique_to_name = {}
    for it in wfstat:
        u = it.get("uniqueName")
        n = it.get("name")
        if u and n:
            unique_to_name[u] = n

    print("Loading warframe.market items (for name -> slug resolution)...")
    wfm = session.get(WFM_ITEMS_URL, timeout=60).json()
    wfm_list = wfm.get("data") or wfm.get("payload") or wfm
    name_to_slug = {}
    valid_slugs = set()
    for it in wfm_list:
        slug = it.get("slug")
        if not slug:
            continue
        valid_slugs.add(slug)
        i18n = (it.get("i18n") or {}).get("en") or {}
        nm = i18n.get("name")
        if nm:
            name_to_slug[nm.lower()] = slug

    print(f"  {len(unique_to_name):,} item paths, {len(name_to_slug):,} market slugs\n")
    return unique_to_name, name_to_slug, valid_slugs


def to_slug_guess(name):
    """WFM slugs are usually lower_snake_case of the display name."""
    s = re.sub(r"[^a-zA-Z0-9 ]", "", name).strip().lower()
    return re.sub(r"\s+", "_", s)


def resolve_to_slug(unique_path, unique_to_name, name_to_slug):
    name = unique_to_name.get(unique_path)
    if not name:
        for suffix in ("Component", "Blueprint"):
            if unique_path.endswith(suffix):
                trimmed = unique_path[: -len(suffix)]
                name = unique_to_name.get(trimmed)
                if name:
                    break
    if not name:
        return None, None
    slug = name_to_slug.get(name.lower()) or to_slug_guess(name)
    return name, slug


def flatten_inventory(inv):
    for cat in TRADEABLE_CATEGORIES:
        for entry in inv.get(cat, []) or []:
            path = entry.get("ItemType") or entry.get("Type")
            count = entry.get("ItemCount", 1)
            if path:
                yield cat, path, count


def load_market_csv(csv_path):
    rows = {}
    with open(csv_path, newline="", encoding="utf-8") as fh:
        for r in csv.DictReader(fh):
            rows[r["url_name"]] = r
    return rows


def _wfm_get(session, path, retries=3):
    """Same backoff pattern as wfm_demand.py — handles 429s and transient errors."""
    for attempt in range(retries):
        try:
            r = session.get(f"{WFM_API}{path}", timeout=30)
            if r.status_code == 429:
                time.sleep(2 ** attempt)
                continue
            r.raise_for_status()
            body = r.json()
            return body.get("payload") or body.get("data") or body
        except requests.RequestException:
            if attempt == retries - 1:
                return None
            time.sleep(2 ** attempt)
    return None


def fetch_live_metrics(session, slug):
    """Return a dict matching wfm_results.csv row shape, or None."""
    orders = _wfm_get(session, f"/v2/orders/item/{slug}")
    time.sleep(REQUEST_DELAY)
    stats = _wfm_get(session, f"/v1/items/{slug}/statistics")
    time.sleep(REQUEST_DELAY)
    if orders is None or not stats:
        return None

    def live(o, kind):
        return (
            o.get("type") == kind
            and (o.get("user") or {}).get("status") in ("ingame", "online")
            and o.get("visible", True)
        )
    live_buys = [o for o in orders if live(o, "buy")]
    live_sells = [o for o in orders if live(o, "sell")]

    recent = stats.get("statistics_closed", {}).get("48hours", [])
    vol = sum(d.get("volume", 0) for d in recent)
    avg = (sum(d.get("avg_price", 0) * d.get("volume", 0) for d in recent) / vol) if vol else 0.0
    top_buy = max((o["platinum"] for o in live_buys), default=0)
    low_sell = min((o["platinum"] for o in live_sells), default=0)
    if live_sells:
        ratio = len(live_buys) / len(live_sells)
    else:
        ratio = len(live_buys) * 10.0
    return {
        "url_name": slug,
        "live_buys": len(live_buys),
        "live_sells": len(live_sells),
        "buy_sell_ratio": round(ratio, 2),
        "top_buy_price": top_buy,
        "low_sell_price": low_sell,
        "volume_48h": vol,
        "avg_price_48h": round(avg, 1),
    }


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--inventory", default="inventory.json",
                   help="Path to inventory.json (default: ./inventory.json)")
    p.add_argument("--csv", default="wfm_results.csv",
                   help="warframe.market data from wfm_demand.py (ignored with --live)")
    p.add_argument("--live", action="store_true",
                   help="Fetch market data live from WFM for owned items only "
                        "(politer than re-scanning the whole catalog)")
    p.add_argument("--top", type=int, default=40, help="Top N rows to print")
    p.add_argument("--min-price", type=float, default=5.0,
                   help="Hide items with avg < this many plat (default: 5)")
    p.add_argument("--out", default="wfm_inventory_sellable.csv",
                   help="Output CSV path")
    args = p.parse_args()

    inv_path = Path(args.inventory)
    if not inv_path.exists():
        print(f"\nNo inventory file at {inv_path.resolve()}\n")
        print("To produce one, run the companion CLI with Warframe open:")
        print("    companion/target/release/wfm-fetch-inventory  (needs sudo on Linux)\n")
        sys.exit(1)

    try:
        inv = json.loads(inv_path.read_text())
    except json.JSONDecodeError as e:
        sys.exit(f"Could not parse {inv_path}: {e}")
    print(f"Loaded {inv_path} ({inv_path.stat().st_size:,} bytes)")

    session = requests.Session()
    session.headers.update({
        "User-Agent": "wfm-inventory/1.0",
        "Platform": "pc",
        "Language": "en",
    })

    unique_to_name, name_to_slug, valid_slugs = build_name_maps(session)

    owned = defaultdict(lambda: {"count": 0, "name": None, "category": None})
    unresolved_by_cat = defaultdict(int)

    for cat, path, count in flatten_inventory(inv):
        name, slug = resolve_to_slug(path, unique_to_name, name_to_slug)
        if not slug:
            unresolved_by_cat[cat] += 1
            continue
        rec = owned[slug]
        rec["count"] += count
        rec["name"] = name
        rec["category"] = cat

    print(f"Resolved {len(owned):,} unique owned items")
    if unresolved_by_cat:
        breakdown = ", ".join(f"{c}:{n}" for c, n in unresolved_by_cat.items())
        print(f"Unresolved (no warframestat.us match): {breakdown}")
    print()

    if args.live:
        # Live mode: only hit WFM for slugs that actually exist there.
        targets = [(s, r) for s, r in owned.items() if s in valid_slugs]
        skipped_unknown = len(owned) - len(targets)
        est = len(targets) * REQUEST_DELAY * 2
        print(f"Live mode: {len(targets)} owned items present in WFM catalog "
              f"({skipped_unknown} skipped as not-tradeable)")
        print(f"  ~{len(targets) * 2} API calls @ {REQUEST_DELAY}s = ~{est/60:.1f} min\n")

        market = {}
        started = time.time()
        for i, (slug, rec) in enumerate(targets, 1):
            m = fetch_live_metrics(session, slug)
            if m:
                market[slug] = m
            if i % 25 == 0 or i == len(targets):
                elapsed = time.time() - started
                rate = i / elapsed if elapsed else 0
                eta = (len(targets) - i) / rate if rate else 0
                print(f"  [{i}/{len(targets)}] kept={len(market)}  "
                      f"elapsed={elapsed:.0f}s  eta={eta:.0f}s  last={slug}")
        print()
    else:
        market = load_market_csv(args.csv)
        print(f"Loaded {len(market):,} market rows from {args.csv}\n")

    sellable = []
    for slug, rec in owned.items():
        m = market.get(slug)
        if not m:
            continue
        avg = float(m.get("avg_price_48h") or 0)
        if avg < args.min_price:
            continue
        vol = int(m.get("volume_48h") or 0)
        sellable.append({
            "slug": slug,
            "name": rec["name"],
            "owned": rec["count"],
            "category": rec["category"],
            "avg_price": avg,
            "low_sell": int(m.get("low_sell_price") or 0),
            "top_buy": int(m.get("top_buy_price") or 0),
            "volume_48h": vol,
            "ratio": float(m.get("buy_sell_ratio") or 0),
            "potential_plat": rec["count"] * avg,
        })

    sellable.sort(key=lambda r: r["potential_plat"], reverse=True)

    if not sellable:
        print("No owned items matched anything in your market CSV above --min-price.")
        return

    with open(args.out, "w", newline="", encoding="utf-8") as fh:
        w = csv.DictWriter(fh, fieldnames=list(sellable[0].keys()))
        w.writeheader()
        w.writerows(sellable)
    print(f"Wrote {len(sellable)} rows to {args.out}\n")

    n = min(args.top, len(sellable))
    print(f"Top {n} owned items by potential plat (owned * avg 48h price):\n")
    header = f"{'Item':<40} {'Own':>4} {'Avg':>6} {'Low':>5} {'Buy':>5} {'Vol':>5} {'Ratio':>5} {'Pot':>8}"
    print(header)
    print("-" * len(header))
    for r in sellable[:n]:
        print(f"{(r['name'] or r['slug'])[:40]:<40} "
              f"{r['owned']:>4} {r['avg_price']:>6.0f} {r['low_sell']:>5} "
              f"{r['top_buy']:>5} {r['volume_48h']:>5} {r['ratio']:>5.2f} "
              f"{r['potential_plat']:>8.0f}")


if __name__ == "__main__":
    try:
        main()
    except KeyboardInterrupt:
        print("\nInterrupted.", file=sys.stderr)
        sys.exit(130)
