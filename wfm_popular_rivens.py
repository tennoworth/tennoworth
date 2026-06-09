#!/usr/bin/env python3
"""
List the most popular riven auctions on warframe.market right now,
filtered to in-game / online sellers with a buyout >= MIN_PLAT.

Usage:
    pip install requests
    python wfm_popular_rivens.py
    python wfm_popular_rivens.py --min 300 --include-online-only

Notes:
- The /v1/auctions/popular endpoint returns the most popular auctions of the last ~4 hours.
- Seller status values: "ingame" (in the game now, can trade), "online" (logged into site only), "offline".
  Default = ingame only, since those are the ones you can actually trade with.
- Rate limit on warframe.market is ~3 req/sec; this script makes 1 request.
"""

import argparse
import sys
from typing import Any

try:
    import requests
except ImportError:
    sys.exit("Install requests first:  pip install requests")

API = "https://api.warframe.market/v1/auctions/popular"
HEADERS = {"Platform": "pc", "Language": "en", "Accept": "application/json"}


def fetch_popular() -> list[dict[str, Any]]:
    r = requests.get(API, headers=HEADERS, timeout=15)
    r.raise_for_status()
    payload = r.json().get("payload", {})
    # The popular endpoint returns auctions under various keys depending on api version.
    # Try the most common shapes.
    for key in ("auctions", "popular", "results"):
        if key in payload:
            return payload[key]
    # Some API versions nest by type:
    if isinstance(payload, dict) and "riven" in payload:
        return payload["riven"]
    return []


def is_riven(a: dict[str, Any]) -> bool:
    t = a.get("item", {}).get("type") or a.get("type")
    return t == "riven"


def seller_ok(a: dict[str, Any], include_online: bool) -> bool:
    status = a.get("owner", {}).get("status", "offline")
    if status == "ingame":
        return True
    if include_online and status == "online":
        return True
    return False


def buyout(a: dict[str, Any]) -> int | None:
    bp = a.get("buyout_price")
    return int(bp) if bp is not None else None


def describe_riven(a: dict[str, Any]) -> str:
    item = a.get("item", {})
    weapon = item.get("weapon_url_name", "?").replace("_", " ").title()
    name = item.get("name", "")        # e.g. "Visi-tron"
    mod_rank = item.get("mod_rank", 0)
    re_rolls = item.get("re_rolls", "?")
    polarity = item.get("polarity", "?")
    mastery = item.get("mastery_level", "?")
    attrs = item.get("attributes", []) or []
    pretty_attrs = []
    for at in attrs:
        val = at.get("value")
        eff = at.get("url_name", at.get("effect", "?")).replace("_", " ")
        sign = "+" if val and val > 0 else ""
        pretty_attrs.append(f"{sign}{val} {eff}")
    stats = " / ".join(pretty_attrs) if pretty_attrs else "(no attrs listed)"
    return (
        f"{weapon} {name}  [MR{mastery}, rank {mod_rank}, {re_rolls} rerolls, {polarity}]\n"
        f"    {stats}"
    )


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--min", type=int, default=100, help="Minimum buyout in platinum (default 100)")
    ap.add_argument(
        "--include-online-only",
        action="store_true",
        help="Also include sellers who are 'online' (site only, not in-game). "
             "Default is in-game only.",
    )
    ap.add_argument("--limit", type=int, default=50, help="Max rows to print")
    args = ap.parse_args()

    try:
        auctions = fetch_popular()
    except requests.HTTPError as e:
        sys.exit(f"API error: {e}")
    except requests.RequestException as e:
        sys.exit(f"Network error: {e}")

    rivens = [a for a in auctions if is_riven(a) and not a.get("closed", False)]
    filtered = [
        a for a in rivens
        if seller_ok(a, args.include_online_only)
        and (buyout(a) is not None and buyout(a) >= args.min)
    ]
    filtered.sort(key=lambda a: buyout(a) or 0, reverse=True)

    print(f"Popular riven auctions  -  min {args.min}p  -  "
          f"sellers: {'ingame+online' if args.include_online_only else 'ingame only'}")
    print(f"{len(filtered)} of {len(rivens)} popular rivens match.\n")

    for a in filtered[: args.limit]:
        seller = a.get("owner", {})
        print(f"{buyout(a):>5}p   @{seller.get('ingame_name', '?')}  ({seller.get('status')})")
        print(f"        {describe_riven(a)}")
        print(f"        https://warframe.market/auction/{a.get('id', '')}")
        print()


if __name__ == "__main__":
    main()
