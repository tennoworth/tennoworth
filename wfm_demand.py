#!/usr/bin/env python3
"""
warframe.market demand analyzer
--------------------------------
Scans warframe.market's public API and ranks tradable items by a composite
"worth farming right now" score.

Usage:
    pip install requests
    python wfm_demand.py                          # scan all 'prime' items (default)
    python wfm_demand.py --filter mod             # scan only mods
    python wfm_demand.py --filter "" --limit 200  # scan first 200 of everything
    python wfm_demand.py --platform ps4
    python wfm_demand.py --min-volume 10 --top 50

Scoring (per item):
    score = volume_48h * avg_price_48h * (1 + live_buy_sell_ratio)

Edit SCORE() at the bottom of analyze_item() to weight things differently.
"""

import argparse
import csv
import json
import os
import statistics
import sys
import time
from datetime import datetime, timezone

import requests

API_ROOT = "https://api.warframe.market"
REQUEST_DELAY = 0.34  # ~3 req/sec — stay under their rate limit


def fetch_json(session, path, retries=3):
    """GET with simple exponential backoff on 429s and transient errors.

    Handles both legacy v1 responses ({"payload": ...}) and v2 responses
    ({"apiVersion": ..., "data": ...}) by returning whichever envelope is present.
    """
    for attempt in range(retries):
        try:
            r = session.get(f"{API_ROOT}{path}", timeout=30)
            if r.status_code == 429:
                time.sleep(2 ** attempt)
                continue
            r.raise_for_status()
            body = r.json()
            if "payload" in body:
                return body["payload"]
            if "data" in body:
                return body["data"]
            return body
        except requests.RequestException:
            if attempt == retries - 1:
                return None
            time.sleep(2 ** attempt)
    return None


def get_all_items(session):
    # /v1/items was retired; v2 returns a flat list under "data".
    data = fetch_json(session, "/v2/items")
    return data or []


def _item_name(item):
    return (item.get("i18n") or {}).get("en", {}).get("name") or item.get("slug", "?")


# warframe.market caps order prices, and low-attention junk items (fish, etc.)
# get wash-traded at that cap to fake a high "last sold" value — e.g. Goopolla
# showed 10 closed trades at 99,999p on a fish whose real price is 1p. One such
# day poisons median / avg / donch for the whole item (it surfaced as a fake
# +1100% in the Trending view). Drop daily closed-stat rows that are cap-pinned
# or an extreme outlier vs the rest of the series before computing any aggregate.
PLAT_CAP = 99_999
OUTLIER_FACTOR = 50  # a day > 50x the cleaned baseline is almost certainly faked


def drop_poisoned_rows(rows):
    """Strip wash-trade / cap-pinned daily rows from a closed-stats list.

    When the cap filter removes EVERYTHING, the item's only activity is
    manipulation — return the empty list so volume sums to 0 and the item
    falls out at the --min-volume gate. (An earlier `or rows` fallback
    handed the raw cap-pinned rows back in exactly that case, which put
    the fabricated 99,999p average at the top of the score sort.)"""
    clean = [
        d for d in rows
        if (d.get("median") or 0) < PLAT_CAP * 0.9
        and (d.get("max_price") or 0) < PLAT_CAP * 0.9
    ]
    meds = sorted(m for d in clean if (m := d.get("median") or 0) > 0)
    if meds:
        baseline = meds[len(meds) // 2]
        if baseline > 0:
            clean = [d for d in clean if (d.get("median") or 0) <= baseline * OUTLIER_FACTOR]
    return clean


def rank0_rows(rows):
    """Keep the unranked tier of a closed-stats list.

    WFM emits one row per (day, mod_rank) for mods — an unranked AND a
    max-rank tier. Baro sells mods unranked and that's the tier players
    resell, so stats must describe rank 0. Items without rank tiers
    (weapons, sets, relics) lack the key entirely, so ABSENCE OF METADATA
    is the only correct fallback condition: an empty result here means
    every trade in the window was max-rank, and the honest output is
    "no rank-0 activity" (vol 0), not max-rank prices wearing a rank-0
    label. (The old `or rows_all` fallback fired exactly in that case,
    reinstating the 2-3x Primed-mod inflation it existed to prevent.)"""
    if not any("mod_rank" in d for d in rows):
        return rows
    return [d for d in rows if (d.get("mod_rank") or 0) == 0]


def canonical_subtype(rows):
    """The single subtype tier this item's stats should describe, or None.

    Relics have the same per-(day, subtype) duality mods have with rank:
    one radiant day at 120p blending into 5-14p intact days fabricated a
    +991% trend and a "peak" badge on meso_l1_relic. Prefer 'intact'
    (the tier relics drop as and overwhelmingly trade as); for items
    whose subtypes don't include intact (gems, fish), use the
    dominant-by-volume tier so the series never mixes price scales."""
    vols = {}
    for d in rows:
        s = d.get("subtype")
        if s:
            vols[s] = vols.get(s, 0) + (d.get("volume") or 0)
    if not vols:
        return None
    return "intact" if "intact" in vols else max(vols, key=lambda s: vols[s])


def subtype_rows(rows, pick):
    """Filter a stats/order list to one subtype tier (None = no-op).
    Rows without a subtype are generic — keep them."""
    if pick is None:
        return rows
    return [d for d in rows if (d.get("subtype") or pick) == pick]


def series_stats(nineties):
    """(median_now, median_90d, medians_7d, donch_top, donch_bot) from an
    already tier-narrowed, poison-filtered 90-day series.

    median_now = the latest day's median — "what it trades at today".
    median_90d = the median OF the daily medians — the 90-day baseline.
    The band is recomputed from the filtered daily medians, not WFM's
    precomputed donch_top/bot — those still reflect a cap-pinned day even
    after we drop it from the series. median_now is one of these medians,
    so it always sits inside [donch_bot, donch_top]."""
    daily_medians = [d.get("median", 0) or 0 for d in nineties]
    medians_7d = daily_medians[-7:]
    if not nineties:
        return 0, 0, medians_7d, 0, 0
    median_now = nineties[-1].get("median", 0) or 0
    nonzero = [m for m in daily_medians if m > 0]
    median_90d = statistics.median(nonzero) if nonzero else median_now
    band = nonzero or [median_now]
    return median_now, median_90d, medians_7d, max(band), min(band)


def analyze_item(session, item):
    """Return a metrics dict for one item, or None on failure."""
    slug = item["slug"]

    orders = fetch_json(session, f"/v2/orders/item/{slug}")
    time.sleep(REQUEST_DELAY)
    # Closed-trade statistics still live on v1; v2 has no equivalent yet.
    stats_payload = fetch_json(session, f"/v1/items/{slug}/statistics")
    time.sleep(REQUEST_DELAY)

    if orders is None or not stats_payload:
        return None

    # 48h closed stats = trades that actually completed. Both windows are
    # narrowed to ONE tier — rank 0 for mods, one subtype for relics/gems —
    # before any aggregate, so avg/median/donch never mix price scales.
    recent_all = [d for d in stats_payload.get("statistics_closed", {}).get("48hours", [])
                  if isinstance(d, dict)]
    nineties_all = [d for d in stats_payload.get("statistics_closed", {}).get("90days", [])
                    if isinstance(d, dict)]
    sub_pick = canonical_subtype(recent_all + nineties_all)

    # Only count orders from players currently reachable in-game/online —
    # offline listings rarely close, so they're noise for "what's selling now."
    # Orders are tier-filtered like the stats: a radiant relic buy order must
    # not become the top_buy quoted against an intact-tier median.
    def live(o, kind):
        return (
            o.get("type") == kind
            and (o.get("user") or {}).get("status") in ("ingame", "online")
            and o.get("visible", True)
        )

    live_buys = subtype_rows([o for o in orders if live(o, "buy")], sub_pick)
    live_sells = subtype_rows([o for o in orders if live(o, "sell")], sub_pick)

    recent = drop_poisoned_rows(subtype_rows(rank0_rows(recent_all), sub_pick))
    volume_48h = sum(d.get("volume", 0) for d in recent)
    if volume_48h > 0:
        avg_price_48h = (
            sum(d.get("avg_price", 0) * d.get("volume", 0) for d in recent) / volume_48h
        )
    else:
        avg_price_48h = 0.0

    # 90d series — the WFM frontend prices against this, not 48h avg, because
    # 48h is noisy on low-volume items. We emit two distinct numbers so the UI's
    # two questions don't collapse into one (they used to, which made the "Δ 90d"
    # trend column structurally ~0):
    #   median_now = the latest day's median — "what it trades at today",
    #   median_90d = the median OF the daily medians — the 90-day baseline.
    # Δ vs 90d = (median_now - median_90d) / median_90d, and the timing band
    # positions median_now inside [donch_bot, donch_top].
    #
    # Same tier-narrowing as the 48h window (see rank0_rows / canonical_subtype
    # for why the old fallbacks were wrong): the raw series mixes rank tiers for
    # mods and refinement tiers for relics — medians_7d alternated tiers, and on
    # a day with only a maxed/radiant trade, "latest" silently grabbed the wrong
    # price scale (Primed Shotgun Ammo Mutation read 160p vs ~45p unranked;
    # meso_l1_relic read Δ+991% off one radiant day).
    nineties = drop_poisoned_rows(subtype_rows(rank0_rows(nineties_all), sub_pick))
    median_now, median_90d, medians_7d, donch_top_90d, donch_bot_90d = series_stats(nineties)

    top_buy = max((o["platinum"] for o in live_buys), default=0)
    low_sell = min((o["platinum"] for o in live_sells), default=0)

    # If there are buyers but no sellers, treat as very high demand pressure.
    if live_sells:
        ratio = len(live_buys) / len(live_sells)
    else:
        ratio = len(live_buys) * 10.0  # arbitrary boost; no competition

    # ---- SCORE() — tweak this to match what you care about ----
    score = volume_48h * avg_price_48h * (1 + ratio)
    # -----------------------------------------------------------

    return {
        "url_name": slug,
        "name": _item_name(item),
        "tags": item.get("tags") or [],
        "ducats": item.get("ducats"),
        "live_buys": len(live_buys),
        "live_sells": len(live_sells),
        "buy_sell_ratio": round(ratio, 2),
        "top_buy_price": top_buy,
        "low_sell_price": low_sell,
        "spread": (low_sell - top_buy) if (low_sell and top_buy) else 0,
        "volume_48h": volume_48h,
        "avg_price_48h": round(avg_price_48h, 1),
        "median_now": round(median_now, 1),
        "median_90d": round(median_90d, 1),
        "medians_7d": medians_7d,
        "donch_top_90d": donch_top_90d,
        "donch_bot_90d": donch_bot_90d,
        "score": round(score, 1),
    }


def build_snapshot(sorted_rows, *, platform, catalog, final):
    """Pure: turn a list of analyzed rows into the JSON shape the web UI consumes."""
    return {
        "updated_at": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "platform": platform,
        "item_count": len(sorted_rows),
        "catalog_count": len(catalog),
        "catalog": catalog,
        "items": {
            r["url_name"]: {
                "avg": r["avg_price_48h"],
                "low_sell": r["low_sell_price"],
                "top_buy": r["top_buy_price"],
                "vol": r["volume_48h"],
                "ratio": r["buy_sell_ratio"],
                "buys": r["live_buys"],
                "sells": r["live_sells"],
                # Extended fields (populated by wfm_demand.py 2026-05+).
                # Older market.json files won't have them; browser code must
                # treat them as optional.
                "tags": r.get("tags") or [],
                "ducats": r.get("ducats"),
                "median_now": r.get("median_now", 0),
                "median_90d": r.get("median_90d", 0),
                "medians_7d": r.get("medians_7d") or [],
                "donch_top_90d": r.get("donch_top_90d", 0),
                "donch_bot_90d": r.get("donch_bot_90d", 0),
            }
            for r in sorted_rows
        },
        "partial": not final,
    }


def write_snapshot(results, *, csv_path, json_path, platform, catalog, final):
    """Write CSV (and JSON if json_path) atomically via tmp + os.replace.
    Concurrent readers — including the live web UI — never see a half-written file."""
    if not results:
        return
    sorted_rows = sorted(results, key=lambda r: r["score"], reverse=True)

    csv_tmp = f"{csv_path}.tmp"
    with open(csv_tmp, "w", newline="", encoding="utf-8") as f:
        writer = csv.DictWriter(f, fieldnames=list(sorted_rows[0].keys()))
        writer.writeheader()
        writer.writerows(sorted_rows)
    os.replace(csv_tmp, csv_path)

    if json_path:
        snapshot = build_snapshot(sorted_rows, platform=platform, catalog=catalog, final=final)
        json_tmp = f"{json_path}.tmp"
        with open(json_tmp, "w", encoding="utf-8") as f:
            json.dump(snapshot, f, separators=(",", ":"))
        os.replace(json_tmp, json_path)


def main():
    p = argparse.ArgumentParser(description="Find high-demand warframe.market items.")
    p.add_argument("--filter", default="prime",
                   help="Case-insensitive substring filter on url_name (default: 'prime'). "
                        "Use --filter '' to scan everything.")
    p.add_argument("--exclude", default="set",
                   help="Substring to exclude (default: 'set' — full prime sets sell less "
                        "than individual parts). Use --exclude '' to disable.")
    p.add_argument("--platform", default="pc",
                   choices=["pc", "ps4", "xbox", "switch"], help="Platform.")
    p.add_argument("--limit", type=int, default=0,
                   help="Limit number of items scanned (0 = no limit).")
    p.add_argument("--min-volume", type=int, default=5,
                   help="Drop items with 48h volume below this (default: 5).")
    p.add_argument("--out", default="wfm_results.csv", help="CSV output path.")
    p.add_argument("--json-out", default=None,
                   help="Also emit a compact JSON snapshot (for the web UI).")
    p.add_argument("--checkpoint-every", type=int, default=100,
                   help="Flush CSV/JSON after every N items (0=only at the end).")
    p.add_argument("--top", type=int, default=25, help="Top N to print to terminal.")
    args = p.parse_args()

    session = requests.Session()
    # WFM's Cloudflare layer 1015-rate-limits generic UAs (see scripts/CLAUDE.md);
    # a real browser UA is required. The old "wfm-demand-analyzer/1.0" survived on
    # GH Actions datacenter IPs but risks a 1015 block from a residential IP — and
    # this scraper now runs on the user's home box. Match scripts/wfm_demand's rule.
    session.headers.update({
        "User-Agent": "Mozilla/5.0 (X11; Linux x86_64; rv:128.0) Gecko/20100101 Firefox/128.0",
        "Platform": args.platform,
        "Language": "en",
    })

    print(f"[{datetime.now():%H:%M:%S}] Fetching master item list...")
    items = get_all_items(session)
    if not items:
        print("Failed to fetch item list. Network problem?", file=sys.stderr)
        sys.exit(1)
    print(f"  Total tradable items: {len(items)}")

    # Build name -> slug catalog from the unfiltered master list — the web UI
    # needs this to resolve any owned item to a WFM slug without calling WFM.
    full_catalog = {}
    for it in items:
        slug = it.get("slug")
        nm = _item_name(it)
        if slug and nm and nm != "?":
            full_catalog[nm.lower()] = slug

    if args.filter:
        f = args.filter.lower()
        items = [i for i in items if f in i["slug"].lower()]
        print(f"  After --filter '{args.filter}': {len(items)} items")
    if args.exclude:
        x = args.exclude.lower()
        items = [i for i in items if x not in i["slug"].lower()]
        print(f"  After --exclude '{args.exclude}': {len(items)} items")
    if args.limit > 0:
        items = items[: args.limit]
        print(f"  Limited to first {args.limit} items")

    est_seconds = len(items) * REQUEST_DELAY * 2
    print(f"  Estimated runtime: ~{est_seconds / 60:.1f} minutes "
          f"({len(items) * 2} API calls @ {REQUEST_DELAY}s each)\n")

    def flush(results, final=False):
        write_snapshot(
            results,
            csv_path=args.out,
            json_path=args.json_out,
            platform=args.platform,
            catalog=full_catalog,
            final=final,
        )

    results = []
    started = time.time()
    for i, item in enumerate(items, 1):
        r = analyze_item(session, item)
        if r and r["volume_48h"] >= args.min_volume:
            results.append(r)
        if i % 25 == 0 or i == len(items):
            elapsed = time.time() - started
            rate = i / elapsed if elapsed else 0
            eta = (len(items) - i) / rate if rate else 0
            print(f"  [{i}/{len(items)}] kept={len(results)}  "
                  f"elapsed={elapsed:.0f}s  eta={eta:.0f}s  last={item['slug']}",
                  flush=True)
        if args.checkpoint_every and i % args.checkpoint_every == 0:
            flush(results, final=False)

    if not results:
        print("\nNo items matched your criteria. Try lowering --min-volume.")
        return

    flush(results, final=True)
    print(f"\nWrote {len(results)} rows to {args.out}")
    if args.json_out:
        print(f"Wrote JSON snapshot to {args.json_out}")

    results.sort(key=lambda r: r["score"], reverse=True)

    # Terminal preview
    n = min(args.top, len(results))
    print(f"\nTop {n} farm targets (by composite score):\n")
    header = f"{'Item':<42} {'Vol48h':>7} {'AvgP':>6} {'Buys':>5} {'Sells':>5} {'Ratio':>6} {'Score':>9}"
    print(header)
    print("-" * len(header))
    for r in results[:n]:
        print(f"{r['name'][:42]:<42} {r['volume_48h']:>7} {r['avg_price_48h']:>6.0f} "
              f"{r['live_buys']:>5} {r['live_sells']:>5} {r['buy_sell_ratio']:>6.2f} "
              f"{r['score']:>9.0f}")


if __name__ == "__main__":
    main()
