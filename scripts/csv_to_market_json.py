#!/usr/bin/env python3
"""
One-shot bootstrap: build prototype/public/market.json from
  - the existing wfm_results.csv (for stats)
  - a fresh fetch of warframe.market /v2/items (for the master catalog)

The catalog lets the browser resolve any owned item to its WFM slug without
calling WFM directly (which can't be done from a browser — no CORS headers).
"""
import csv
import json
import sys
import time
from datetime import datetime, timezone
from pathlib import Path

import requests

# Cloudflare 1015-blocks generic UAs (scripts/CLAUDE.md hard rule). Same
# string as wfm_demand.py — keep them in sync.
HEADERS = {
    "User-Agent": "Mozilla/5.0 (X11; Linux x86_64; rv:128.0) Gecko/20100101 Firefox/128.0",
}


def _get(url, timeout=30):
    return requests.get(url, timeout=timeout, headers=HEADERS)

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
CSV_IN = ROOT / "wfm_results.csv"
JSON_OUT = ROOT / "prototype" / "public" / "market.json"
WFM_ITEMS_URL = "https://api.warframe.market/v2/items"
# warframestat exposes prime PARTS only nested under each parent item's
# `components[]`. Its bulk /items/ endpoint omits them entirely, which means
# the browser's resolver (which uses /items/) can't see paths like
# /Lotus/Types/Recipes/WarframeRecipes/VoltPrimeChassisComponent. We
# pre-walk the parent categories here and bake a path→{name, slug, category}
# map into market.json so the resolver can resolve component paths
# directly.
WFSTAT_PARENT_ENDPOINTS = [
    ("https://api.warframestat.us/warframes/", "Warframes"),
    ("https://api.warframestat.us/weapons/", "Weapons"),
    ("https://api.warframestat.us/sentinels/", "Sentinels"),
]
# Drop tables for relics → rewards. We only ingest the `Intact` state
# for v1 — it's the most common refinement to crack raw. Radiant /
# Flawless / Exceptional get different drop weights but the user has to
# pre-refine for those, which is its own decision.
WFSTAT_RELICS_URL = "https://drops.warframestat.us/data/relics.json"
# Baro Ki'Teer's schedule. Baked at build time so the browser's Baro view
# never fetches warframestat directly — that violated the resolver-only
# rule and vanished during warframestat outages.
WFSTAT_VOIDTRADER_URL = "https://api.warframestat.us/pc/voidTrader/"
# Bulk item list backing the browser's /Lotus/... path resolver. The
# browser used to fetch this directly (warframestat sent CORS headers);
# upstream dropped Access-Control-Allow-Origin on 2026-06-09, so the slim
# (uniqueName → name/category) pairs are baked here and served same-origin
# like every other vendor dataset.
WFSTAT_ITEMS_URL = "https://api.warframestat.us/items/"
CATALOG_OUT = ROOT / "prototype" / "public" / "wfstat-catalog.json"
# Vault status lives on WFCD's warframe-items dataset per-parent-prime
# (Warframes.json + Weapons.json). Each parent has `vaulted: bool` and
# `estimatedVaultDate: ISO`. We propagate from parent → all its
# component slugs so a single chip can answer "is this on the vault
# cliff?" for any prime part in the table.
# WFCD splits weapons across category-specific JSONs. We walk all of
# them so prime weapon parts (e.g. Akstiletto Prime Barrel) get vault
# status alongside warframes.
WFCD_VAULT_SOURCES = [
    "https://raw.githubusercontent.com/WFCD/warframe-items/master/data/json/Warframes.json",
    "https://raw.githubusercontent.com/WFCD/warframe-items/master/data/json/Primary.json",
    "https://raw.githubusercontent.com/WFCD/warframe-items/master/data/json/Secondary.json",
    "https://raw.githubusercontent.com/WFCD/warframe-items/master/data/json/Melee.json",
    "https://raw.githubusercontent.com/WFCD/warframe-items/master/data/json/Archwing.json",
    "https://raw.githubusercontent.com/WFCD/warframe-items/master/data/json/Arch-Gun.json",
    "https://raw.githubusercontent.com/WFCD/warframe-items/master/data/json/Arch-Melee.json",
    "https://raw.githubusercontent.com/WFCD/warframe-items/master/data/json/SentinelWeapons.json",
    "https://raw.githubusercontent.com/WFCD/warframe-items/master/data/json/Sentinels.json",
    "https://raw.githubusercontent.com/WFCD/warframe-items/master/data/json/Pets.json",
]
# Resurgence horizon: estimated-vault-dates within this window flag the
# part as "vaulting soon" (a sell-signal, not just informational).
VAULT_SOON_DAYS = 60


def _parse_medians(raw):
    """CSV stores the 7-day median series as the Python `repr` of a list
    (csv.DictWriter just calls str() on it). Parse defensively so an old
    CSV with no column at all, or with `[]`, both come back as []."""
    if not raw:
        return []
    try:
        val = json.loads(raw.replace("'", '"'))
        return val if isinstance(val, list) else []
    except (ValueError, TypeError):
        return []


def fetch_vault_status(catalog):
    """Returns ({slug: "vaulted"|"vaulting-soon"|"available"}, complete)
    for every prime part findable across WFCD warframe-items. Walks each
    parent item (warframe / weapon), reads `vaulted` + `estimatedVaultDate`,
    propagates to all component slugs (set, blueprint, each part).

    `complete` is False when ANY source failed — a 9-of-10 fetch yields a
    truthy-but-incomplete map, and per the snapshot contract missing slugs
    read as "available", so overwriting a complete prior with it silently
    strips vault badges (a sell signal). The caller merges with prior in
    that case."""
    from datetime import datetime, timezone, timedelta
    out = {}
    complete = True
    now = datetime.now(timezone.utc)
    soon_cutoff = now + timedelta(days=VAULT_SOON_DAYS)

    for url in WFCD_VAULT_SOURCES:
        try:
            arr = _get(url).json()
        except Exception as e:
            print(f"  warning: could not fetch {url}: {e}")
            complete = False
            continue
        if not isinstance(arr, list):
            complete = False
            continue
        for parent in arr:
            if not isinstance(parent, dict):
                continue
            parent_name = parent.get("name") or ""
            if "Prime" not in parent_name:
                continue
            vaulted = bool(parent.get("vaulted"))
            est = parent.get("estimatedVaultDate")
            soon = False
            if not vaulted and est:
                try:
                    est_dt = datetime.fromisoformat(est.replace("Z", "+00:00"))
                    # Some entries omit the timezone offset — assume UTC
                    # rather than crashing on a naive-vs-aware compare.
                    if est_dt.tzinfo is None:
                        est_dt = est_dt.replace(tzinfo=timezone.utc)
                    if est_dt < soon_cutoff:
                        soon = True
                except (ValueError, AttributeError):
                    pass
            if vaulted:
                status = "vaulted"
            elif soon:
                status = "vaulting-soon"
            else:
                status = "available"

            # Propagate the parent's status to every slug we can reach:
            # the set, the blueprint, and every component-name variant.
            candidate_names = [f"{parent_name} set", f"{parent_name} blueprint"]
            for comp in parent.get("components") or []:
                cn = comp.get("name")
                if not cn:
                    continue
                candidate_names.append(f"{parent_name} {cn}".lower())
                candidate_names.append(f"{parent_name} {cn} blueprint".lower())
            seen = set()
            for nm in candidate_names:
                key = nm.lower()
                slug = catalog.get(key)
                if slug and slug not in seen:
                    out[slug] = status
                    seen.add(slug)
    return out, complete


def fetch_void_trader():
    """Baro Ki'Teer's schedule from warframestat. We bake only the three
    fields the browser renders (activation / expiry / location) — the
    inventory list isn't shown. Empty dict on any failure or missing
    field; the Baro card then hides instead of rendering 'undefined'."""
    try:
        data = _get(WFSTAT_VOIDTRADER_URL).json()
    except Exception as e:
        print(f"  warning: could not fetch {WFSTAT_VOIDTRADER_URL}: {e}")
        return {}
    if not isinstance(data, dict):
        return {}
    activation = data.get("activation")
    expiry = data.get("expiry")
    location = data.get("location")
    if not (activation and expiry and location):
        return {}
    return {"activation": activation, "expiry": expiry, "location": location}


def fetch_relic_rewards(catalog):
    """Returns {relic_slug: [{reward_slug, reward_name, rarity, chance}]}
    from `drops.warframestat.us`. Only the Intact state is captured —
    refined states have their own drop weights, but the user has to
    pre-refine, so cracking-raw is the default question the planner
    answers. Returns {} if the endpoint is unreachable (the relic
    planner UI degrades to an empty-state card).
    """
    try:
        body = _get(WFSTAT_RELICS_URL).json()
    except Exception as e:
        print(f"  warning: could not fetch {WFSTAT_RELICS_URL}: {e}")
        return {}
    rows = body.get("relics") if isinstance(body, dict) else None
    if not isinstance(rows, list):
        print("  warning: relics.json unexpected shape")
        return {}

    out = {}
    for row in rows:
        if not isinstance(row, dict):
            continue
        if row.get("state") != "Intact":
            continue
        tier = (row.get("tier") or "").lower()
        name = (row.get("relicName") or "").lower()
        if not tier or not name:
            continue
        relic_slug = f"{tier}_{name}_relic"
        rewards = []
        for r in row.get("rewards") or []:
            if not isinstance(r, dict):
                continue
            reward_name = r.get("itemName") or ""
            if not reward_name:
                continue
            # WFM's slug for a relic reward is most often the blueprint
            # variant (`<x> blueprint`) for warframe parts, the bare
            # component slug for weapon parts, or "_set" as last resort.
            # Mirror the lookup order used by fetch_parent_data.
            reward_slug = (
                catalog.get(reward_name.lower())
                or catalog.get(f"{reward_name.lower()} blueprint")
            )
            if not reward_slug:
                continue
            rewards.append({
                "reward_slug": reward_slug,
                "reward_name": reward_name,
                "rarity": r.get("rarity") or "",
                "chance": float(r.get("chance") or 0),
            })
        if rewards:
            out[relic_slug] = rewards
    return out


def fetch_parent_data(catalog):
    """Single walk over warframestat parent endpoints producing two maps:
       - `path_to_info`: {component_uniqueName: {name, slug, category}}
       - `set_to_parts`: {set_slug: {name, parts: [{slug, component_name}]}}
    The set map powers the browser's set-completion card without
    requiring another network round-trip. `catalog` is the lowercased
    name → slug map already built from WFM.

    Returns (path_to_info, set_to_parts, complete) — complete is False
    when any endpoint failed, so the caller can merge with the prior
    snapshot instead of replacing a complete map with a partial one
    (which made whole inventory categories unresolvable)."""
    path_to_info = {}
    set_to_parts = {}
    complete = True
    for url, fallback_cat in WFSTAT_PARENT_ENDPOINTS:
        try:
            arr = _get(url).json()
        except Exception as e:
            print(f"  warning: could not fetch {url}: {e}")
            complete = False
            continue
        if not isinstance(arr, list):
            print(f"  warning: {url} returned non-list (skipping)")
            complete = False
            continue
        for parent in arr:
            # Some warframestat endpoints occasionally contain string
            # entries or other non-dict elements; skip defensively.
            if not isinstance(parent, dict):
                continue
            parent_name = parent.get("name") or ""
            parent_cat = parent.get("category") or fallback_cat
            if "Prime" not in parent_name:
                continue
            set_slug = catalog.get(f"{parent_name.lower()} set")
            this_set_parts = []
            for comp in parent.get("components") or []:
                un = comp.get("uniqueName") or ""
                cn = comp.get("name") or ""
                if not un or not cn:
                    continue
                # Skip shared resources (Orokin Cell, Argon Crystal, …) —
                # they aren't unique to this set and the catalog already
                # resolves them by their own name.
                if un.startswith("/Lotus/Types/Items/MiscItems/"):
                    continue
                full_name = f"{parent_name} {cn}"
                # WFM's naming for parts isn't consistent across primes —
                # older sets list "Volt Prime Chassis Blueprint" only,
                # newer ones have both. Try the blueprint variant first
                # because that's the most common dropped form. If still
                # nothing, fall back to "<parent>_set" so we at least
                # surface the row.
                slug = (
                    catalog.get(f"{full_name.lower()} blueprint")
                    or catalog.get(full_name.lower())
                    or set_slug
                )
                if not slug:
                    continue
                # When we fall back to the set, surface a name that doesn't
                # lie about what the user owns — append "(set)" so a "Volt
                # Prime Chassis" row labelled as the set is recognisable.
                display_name = full_name
                if slug.endswith("_set") and not full_name.endswith("Set"):
                    display_name = f"{full_name} → set"
                elif slug.endswith("_blueprint") and not full_name.endswith("Blueprint"):
                    display_name = f"{full_name} Blueprint"
                path_to_info[un] = {
                    "name": display_name,
                    "slug": slug,
                    "category": parent_cat,
                }
                # The "set itself" slug isn't a part of the set — skip
                # the fallback case where slug == set_slug.
                if slug != set_slug:
                    this_set_parts.append({
                        "slug": slug,
                        "component_name": cn,
                    })
            if set_slug and this_set_parts:
                set_to_parts[set_slug] = {
                    "name": parent_name,
                    "parts": this_set_parts,
                }
    return path_to_info, set_to_parts, complete


def fetch_wfstat_slim():
    """Slim [uniqueName, {name, category}] pairs for the browser resolver,
    in the exact shape its IndexedDB cache stores. English is forced —
    the endpoint varies on Accept-Language, and a localized catalog
    silently breaks the name → WFM-slug join (a pt-PT browser produced
    rows like "Liga Metálica" that matched nothing on WFM)."""
    r = requests.get(WFSTAT_ITEMS_URL, timeout=60, headers={**HEADERS, "Accept-Language": "en"})
    r.raise_for_status()
    arr = r.json()
    if not isinstance(arr, list):
        raise ValueError("warframestat /items/ returned non-list")
    slim = []
    for it in arr:
        if isinstance(it, dict) and it.get("uniqueName") and it.get("name"):
            slim.append([it["uniqueName"], {"name": it["name"], "category": it.get("category")}])
    return slim


def fetch_catalog():
    """Returns (catalog, meta_by_slug). `catalog` maps lowercased name → slug
    (used by the browser to resolve an inventory item to a WFM listing).
    `meta_by_slug` maps slug → {tags, ducats, max_rank, subtypes} pulled
    from WFM's authoritative item catalog — used to populate `items[]`
    entries so the browser can filter by category and surface ducat values
    without re-fetching /v2/items.

    Retries with backoff — this script is the sole market.json producer,
    and an unguarded single GET here aborted the whole rebuild (discarding
    a finished 45-min scrape) on one Cloudflare hiccup. Raises only after
    all attempts fail; main() then falls back to the prior snapshot."""
    last_err = None
    for attempt in range(3):
        try:
            r = _get(WFM_ITEMS_URL)
            r.raise_for_status()
            body = r.json()
            break
        except Exception as e:
            last_err = e
            print(f"  warning: WFM catalog attempt {attempt + 1}/3 failed: {e}", flush=True)
            time.sleep(2 * (attempt + 1))
    else:
        raise RuntimeError(f"WFM catalog unreachable after 3 attempts: {last_err}")
    items = body.get("data") or body.get("payload") or body
    catalog = {}
    meta_by_slug = {}
    for it in items:
        slug = it.get("slug")
        nm = (it.get("i18n") or {}).get("en", {}).get("name")
        if slug and nm:
            catalog[nm.lower()] = slug
        if slug:
            meta_by_slug[slug] = {
                "tags": it.get("tags") or [],
                "ducats": it.get("ducats"),
                "max_rank": it.get("maxRank"),
                "subtypes": it.get("subtypes") or [],
            }
    return catalog, meta_by_slug


def main():
    if not CSV_IN.exists():
        sys.exit(f"{CSV_IN} not found — run wfm_demand.py first.")

    # Prior snapshot loads FIRST: it's the fallback for every fetch below
    # (full outage → keep surface; partial outage → merge over prior).
    prior = {}
    if JSON_OUT.exists():
        try:
            prior = json.loads(JSON_OUT.read_text(encoding="utf-8"))
        except (json.JSONDecodeError, OSError):
            prior = {}

    print("Fetching warframe.market master catalog...")
    try:
        catalog, meta_by_slug = fetch_catalog()
        print(f"  {len(catalog):,} items")
    except RuntimeError as e:
        # This script is the sole market.json producer and is documented as
        # "useful when WFM is down" — so a WFM outage must not discard a
        # finished 45-min scrape. Tags/ducats are recoverable from the prior
        # snapshot's own items.
        if not prior.get("catalog"):
            sys.exit(f"{e} — and no prior snapshot to fall back on.")
        print(f"  {e} — reusing the prior snapshot's catalog", flush=True)
        catalog = prior["catalog"]
        meta_by_slug = {
            slug: {"tags": it.get("tags") or [], "ducats": it.get("ducats"),
                   "max_rank": None, "subtypes": []}
            for slug, it in (prior.get("items") or {}).items()
        }

    print("Fetching warframestat component path map + sets...")
    path_to_info, set_to_parts, parents_complete = fetch_parent_data(catalog)
    print(f"  {len(path_to_info):,} component paths · {len(set_to_parts):,} prime sets")

    print("Fetching relic drop tables (Intact)...")
    relic_rewards = fetch_relic_rewards(catalog)
    print(f"  {len(relic_rewards):,} relics with reward data")

    print("Fetching prime vault status...")
    vault_status, vault_complete = fetch_vault_status(catalog)
    counts = {}
    for v in vault_status.values():
        counts[v] = counts.get(v, 0) + 1
    print(f"  {len(vault_status):,} slugs tagged · {counts}")

    print("Fetching Baro Ki'Teer schedule...")
    baro = fetch_void_trader()
    print(f"  baro: {baro.get('location') or 'unavailable'}")

    print("Fetching warframestat bulk item catalog (resolver data)...")
    try:
        wfstat_slim = fetch_wfstat_slim()
    except Exception as e:
        print(f"  warning: could not fetch {WFSTAT_ITEMS_URL}: {e}")
        wfstat_slim = []
    # Preserve-on-empty, same policy as the market.json surfaces: an
    # upstream outage must not blank a previously good catalog.
    if not wfstat_slim and CATALOG_OUT.exists():
        print(f"  fetch empty — keeping existing {CATALOG_OUT.name}", flush=True)
    elif wfstat_slim:
        tmp = CATALOG_OUT.with_name(CATALOG_OUT.name + ".tmp")
        with open(tmp, "w", encoding="utf-8") as fh:
            json.dump(wfstat_slim, fh, separators=(",", ":"))
        tmp.replace(CATALOG_OUT)
        print(f"  {len(wfstat_slim):,} entries → {CATALOG_OUT.name}", flush=True)

    # Preserve-on-outage. warframestat 522s intermittently (caught
    # 2026-05-28: an outage left set_to_parts empty and blanked the Sets
    # card). Empty fetch → keep the prior surface wholesale. PARTIAL fetch
    # (one of N sources failed) → merge fresh over prior: a 9-of-10 vault
    # fetch is truthy but incomplete, and missing slugs implicitly read as
    # "available", so replacing the prior with it silently stripped vault
    # badges. Each surface carries a fetched_at stamp so stale-data
    # survival is visible instead of masked by a fresh updated_at.
    now_iso = datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
    prior_stamps = prior.get("surface_fetched_at") or {}
    surface_fetched_at = {}
    STALE_DAYS = 7

    def reconcile(name, fresh, complete=True):
        old = prior.get(name)
        if not fresh and old:
            kept_since = prior_stamps.get(name, now_iso)
            print(f"  {name}: fetch empty — keeping {len(old):,} from prior snapshot "
                  f"(fetched {kept_since})", flush=True)
            try:
                age = datetime.now(timezone.utc) - datetime.strptime(
                    kept_since, "%Y-%m-%dT%H:%M:%SZ").replace(tzinfo=timezone.utc)
                if age.days >= STALE_DAYS:
                    print(f"  WARNING: {name} has been stale for {age.days} days — "
                          f"upstream looks permanently broken, investigate.", flush=True)
            except ValueError:
                pass
            surface_fetched_at[name] = kept_since
            return old
        if not complete and old:
            merged = {**old, **fresh}
            recovered = len(merged) - len(fresh)
            if recovered > 0:
                print(f"  {name}: partial fetch — kept {recovered:,} entries "
                      f"from prior snapshot", flush=True)
            surface_fetched_at[name] = now_iso
            return merged
        surface_fetched_at[name] = now_iso
        return fresh

    path_to_info = reconcile("path_to_info", path_to_info, parents_complete)
    set_to_parts = reconcile("set_to_parts", set_to_parts, parents_complete)
    relic_rewards = reconcile("relic_rewards", relic_rewards)
    vault_status = reconcile("vault_status", vault_status, vault_complete)
    baro = reconcile("baro", baro)

    items = {}
    with open(CSV_IN, newline="", encoding="utf-8") as fh:
        for r in csv.DictReader(fh):
            slug = r["url_name"]
            meta = meta_by_slug.get(slug, {})
            items[slug] = {
                "avg": float(r["avg_price_48h"] or 0),
                "low_sell": int(r["low_sell_price"] or 0),
                # Avg of the ~5 cheapest live asks (depth-aware current
                # price); 0 on pre-2026-06-10 CSVs — UI falls back to avg.
                "low5_avg": float(r.get("low5_avg") or 0),
                "top_buy": int(r["top_buy_price"] or 0),
                "vol": int(r["volume_48h"] or 0),
                "ratio": float(r["buy_sell_ratio"] or 0),
                "buys": int(r["live_buys"] or 0),
                "sells": int(r["live_sells"] or 0),
                # Extended fields. Tags + ducats come from the live
                # /v2/items pull — WFM is authoritative for both because
                # warframestat's bulk /items/ doesn't carry ducats (those
                # are nested under parent items' `components`).
                # Statistics fields are populated by wfm_demand.py going
                # forward; CSV rebuilds inherit whatever was captured at
                # last full scrape (and 0 / [] when the CSV is older).
                "tags": meta.get("tags", []),
                "ducats": meta.get("ducats"),  # None when not ducat-purchasable
                # median_now = today's median (band positioning); median_90d =
                # the 90-day baseline (Δ-vs-90d). Pre-split CSVs lack median_now
                # → fall back to median_90d so old rebuilds still position sanely.
                "median_now": float(r.get("median_now") or r.get("median_90d") or 0),
                "median_90d": float(r.get("median_90d") or 0),
                "medians_7d": _parse_medians(r.get("medians_7d")),
                "donch_top_90d": int(float(r.get("donch_top_90d") or 0)),
                "donch_bot_90d": int(float(r.get("donch_bot_90d") or 0)),
            }

    snapshot = {
        "updated_at": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "platform": "pc",
        "item_count": len(items),
        "catalog_count": len(catalog),
        "source": "bootstrap from wfm_results.csv + /v2/items",
        "catalog": catalog,
        "items": items,
        # Component paths warframestat's bulk /items/ omits (prime
        # blueprints / chassis / systems / weapon barrels / etc.). The
        # browser resolver consults this map *before* falling back to the
        # warframestat slim cache, so component paths resolve cleanly.
        "path_to_info": path_to_info,
        # Prime-set → constituent parts. Powers the set-completion card
        # (near-complete / extras / duplicate-sets recos). Each value
        # is {name, parts: [{slug, component_name}]}.
        "set_to_parts": set_to_parts,
        # Relic (Intact state only, v1) → rewards. Powers the relic-planner
        # card's "expected plat per crack" calculation. Empty when
        # drops.warframestat is unreachable — UI degrades gracefully.
        "relic_rewards": relic_rewards,
        # Prime-part vault status: "vaulted" / "vaulting-soon" / "available"
        # per slug. Powers the vault badge + "Vaulted only" preset. Missing
        # slugs = "available" implicitly (no badge).
        "vault_status": vault_status,
        # Baro Ki'Teer schedule (activation / expiry / location). Baked so
        # the Baro view needs no runtime warframestat fetch. Empty {} when
        # warframestat is unreachable — the card hides.
        "baro": baro,
        # Per-surface freshness. updated_at refreshes every run even when a
        # surface is riding on preserved prior data — these stamps say when
        # each surface was actually fetched successfully.
        "surface_fetched_at": surface_fetched_at,
    }
    JSON_OUT.parent.mkdir(parents=True, exist_ok=True)
    # Atomic write: the browser reloads market.json live, so it must never
    # observe a half-written file. POSIX rename on the same FS is atomic.
    tmp = JSON_OUT.with_name(JSON_OUT.name + ".tmp")
    with open(tmp, "w", encoding="utf-8") as fh:
        json.dump(snapshot, fh, separators=(",", ":"))
    tmp.replace(JSON_OUT)
    print(f"Wrote {JSON_OUT} ({JSON_OUT.stat().st_size:,} bytes)", flush=True)


if __name__ == "__main__":
    main()
