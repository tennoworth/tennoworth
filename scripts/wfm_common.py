#!/usr/bin/env python3
"""Shared warframe.market HTTP primitives for the Python pipeline.

Both wfm_demand.py (root) and scripts/csv_to_market_json.py talk to WFM and
must present the same browser UA — Cloudflare 1015-blocks generic UAs (see
scripts/CLAUDE.md). Mirrors companion/wfm-client/src/lib.rs's BROWSER_UA,
which mirrors companion/wfm-core's — the value proven against production
traffic. Bump all three together if you ever need to change it.
"""
import time

import requests

API_ROOT = "https://api.warframe.market"

BROWSER_UA = "Mozilla/5.0 (X11; Linux x86_64; rv:130.0) Gecko/20100101 Firefox/130.0"


def wfm_session(platform="pc"):
    """A requests.Session preconfigured with WFM's required headers."""
    session = requests.Session()
    session.headers.update({
        "User-Agent": BROWSER_UA,
        "Platform": platform,
        "Language": "en",
    })
    return session


def fetch_json(session, url, retries=3, timeout=30):
    """GET with exponential backoff on 429s and transient errors.

    Handles both legacy v1 responses ({"payload": ...}) and v2 responses
    ({"apiVersion": ..., "data": ...}) by returning whichever envelope is
    present, else the raw body. Returns None if every attempt fails —
    callers already treat that as "skip this item", matching wfm_demand.py's
    existing behavior.
    """
    for attempt in range(retries):
        try:
            r = session.get(url, timeout=timeout)
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
