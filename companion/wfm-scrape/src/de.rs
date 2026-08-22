//! Digital Extremes first-party ingest — Public Export + worldState.
//!
//! DE publishes no documented API, no keys and no rate limit. These are the
//! unauthenticated endpoints the game, the Companion app and the Arsenal
//! Twitch extension run on; the community has used them for a decade and DE
//! has tolerated it. **Tolerated is not licensed** — so every fetch here goes
//! through the same descriptive User-Agent and once-per-cycle pacing we honour
//! for warframe.market, from the box, never per-visitor.
//!
//! GROUND RULES (they mirror the rest of the crate):
//! - Every URL lives here and nowhere else. Five of the paths the community
//!   wiki documents are already dead, including the worldState URL most guides
//!   still quote; when the next one moves this is the only file to edit.
//! - No clock reads. Timestamps arrive from worldState or from the injected
//!   clock the caller already owns.
//! - Failure is never fatal. Each surface returns empty on error and the
//!   caller reconciles against the prior snapshot, exactly like the
//!   warframestat surfaces do.
//! - Nothing is resolved by guessing. An unresolvable `/Lotus/...` path is
//!   reported, not silently mapped — a wrong price is worse than no price.

use std::collections::{BTreeMap, HashMap};

use serde_json::Value;

use crate::fetch::Http;

// ---------------------------------------------------------------------------
// Endpoints
// ---------------------------------------------------------------------------

/// LZMA-alone compressed newline list of `File.json!hash` entries.
///
/// Note the host: the export tree lives on `content.` and the dynamic PHP on
/// `api.` — they do not cross. `api.warframe.com/cdn/PublicExport/` is a 404.
pub const DE_INDEX_URL: &str = "https://content.warframe.com/PublicExport/index_en.txt.lzma";

/// Manifest base. The hash is **part of the path** — the bare filename 404s.
pub const DE_MANIFEST_BASE: &str = "https://content.warframe.com/PublicExport/Manifest/";

/// Failover origin for the same export tree. The bare
/// `origin.warframe.com/PublicExport/Manifest/` path 403s; the `/origin/00000000/`
/// prefix is required.
pub const DE_ORIGIN_INDEX_URL: &str =
    "https://origin.warframe.com/origin/00000000/PublicExport/index_en.txt.lzma";

/// DE's annual usage telemetry: what players actually equip, by Mastery Rank.
///
/// Six years of it, one static file per year, and nothing in the trading
/// ecosystem reads it. Shape is
/// `{"ALL": {category: {item_name: {"ALL": share, "0".."33": share}}}}` where
/// the numeric keys are Mastery Rank buckets and every value is a fraction of
/// that category's total usage.
///
/// Keyed by DISPLAY NAME, not uniqueName — so it joins through the item
/// catalogue rather than the `/Lotus/...` map, and a renamed weapon simply
/// misses. Annual, so it is fetched once and cached, never per cycle. The
/// current year is published in arrears: 2026's file did not exist in
/// 2026-08, which is why the caller must label the year it is showing.
pub const DE_USAGE_URL_TEMPLATE: &str =
    "https://www-static.warframe.com/repos/WarframeUsageData{year}.json";

/// Years DE has actually published. Probed 2026-08-22: 2020 through 2025
/// exist, 2026 is a 404.
pub const DE_USAGE_YEARS: &[u16] = &[2020, 2021, 2022, 2023, 2024, 2025];

pub fn usage_url(year: u16) -> String {
    DE_USAGE_URL_TEMPLATE.replace("{year}", &year.to_string())
}

/// Live game state. Moved here from `content.warframe.com/dynamic/worldState.php`,
/// which is now a 404. `?platform=` returns 409 — cross-play unified it.
pub const DE_WORLD_STATE_URL: &str = "https://api.warframe.com/cdn/worldState.php";

/// Manifest basenames we actually read. The index lists 16; pulling only these
/// keeps a cold sync to ~8 MB instead of ~14 MB.
pub const WANTED_MANIFESTS: &[&str] = &[
    "ExportWeapons_en.json",
    "ExportRecipes_en.json",
    "ExportRelicArcane_en.json",
    "ExportWarframes_en.json",
    "ExportUpgrades_en.json",
    "ExportResources_en.json",
];

/// Manifests fetched every cycle regardless of their hash.
///
/// THE RULE: a manifest may only be skipped when everything derived from it is
/// a standalone surface that `reconcile` can carry. Two things disqualify one,
/// and both bit us:
///
/// 1. **It feeds an override on a surface rebuilt every cycle.** Dispositions
///    are applied on top of the riven surface (refetched from warframe.market
///    each run) and ducats on top of the item catalogue (likewise). There is
///    nothing to carry — the host surface arrives fresh and simply loses the
///    override — so skipping reverts dispositions to WFM's lagging mirror and
///    ducats to WFM's value, silently, on the very next cycle. The fixture
///    disagrees by design: WFM says 45 for a Volt Prime Chassis Blueprint,
///    DE says 65.
/// 2. **Another surface depends on it.** Relic rewards need `ExportRecipes`
///    to resolve blueprint paths. Caching the two independently means a cycle
///    where only the relic hash moved has no recipes to build with — and the
///    code would fall through to the legacy intact-only source. Keeping the
///    shared dependency always-present removes the hazard rather than
///    coordinating two caches.
///
/// Together these are ~1.9 MB a cycle against a scrape that already runs for
/// half an hour. Correctness is worth more than the saving; the 3.2 MB relic
/// manifest, which is a standalone carryable surface, still skips.
pub const ALWAYS_FETCH: &[&str] = &["ExportWeapons_en.json", "ExportRecipes_en.json"];

// ---------------------------------------------------------------------------
// Index
// ---------------------------------------------------------------------------

/// One line of the export index: the basename and the content hash that must
/// travel with it in the URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexEntry {
    pub file: String,
    pub hash: String,
}

impl IndexEntry {
    /// The path segment DE actually serves: `File.json!hash`.
    pub fn segment(&self) -> String {
        format!("{}!{}", self.file, self.hash)
    }

    pub fn url(&self) -> String {
        format!("{DE_MANIFEST_BASE}{}", self.segment())
    }
}

/// Parse the decompressed index into basename → entry.
///
/// Lines are `ExportWeapons_en.json!00_zwapa0tHPowLsyegwAU-zw`. A line without
/// a `!` is skipped rather than guessed at — a hashless URL 404s, so inventing
/// one would only turn a parse problem into a fetch problem.
pub fn parse_index(text: &str) -> BTreeMap<String, IndexEntry> {
    let mut out = BTreeMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Some((file, hash)) = line.split_once('!') else {
            continue;
        };
        if file.is_empty() || hash.is_empty() {
            continue;
        }
        out.insert(
            file.to_string(),
            IndexEntry { file: file.to_string(), hash: hash.to_string() },
        );
    }
    out
}

/// Decompress a legacy LZMA-alone stream.
///
/// This is **not** xz. A standard xz decoder rejects the index outright: the
/// file carries the 13-byte alone header (properties + dict size + unknown
/// size), not the xz container magic. `lzma_rs::lzma_decompress` is the
/// matching primitive.
pub fn decode_lzma_alone(bytes: &[u8]) -> Result<String, String> {
    let mut reader = std::io::Cursor::new(bytes);
    let mut out = Vec::new();
    lzma_rs::lzma_decompress(&mut reader, &mut out)
        .map_err(|e| format!("LZMA-alone decode failed: {e}"))?;
    String::from_utf8(out).map_err(|e| format!("index is not UTF-8: {e}"))
}

// ---------------------------------------------------------------------------
// Manifests
// ---------------------------------------------------------------------------

/// Parse a manifest body, strictly first.
///
/// All 16 `_en` manifests parsed as strict JSON when this was written
/// (verified 2026-08-22). The sanitising fallback exists because these are
/// internal files DE never promised to keep well-formed, and because raw
/// control characters inside description strings are the failure the community
/// has historically hit on other locales. If the fallback ever fires in
/// production it is worth looking at, so it says so.
pub fn parse_manifest(raw: &str) -> Result<Value, String> {
    match serde_json::from_str::<Value>(raw) {
        Ok(v) => Ok(v),
        Err(strict_err) => {
            let cleaned = sanitize_control_chars(raw);
            match serde_json::from_str::<Value>(&cleaned) {
                Ok(v) => {
                    eprintln!(
                        "  note: manifest needed control-char sanitising (strict parse said: {strict_err})"
                    );
                    Ok(v)
                }
                Err(e) => Err(format!("manifest parse failed: {strict_err} (and after sanitising: {e})")),
            }
        }
    }
}

/// Replace raw control characters that appear *inside* JSON strings with
/// spaces, leaving structural whitespace alone.
///
/// Tracks string state and backslash escaping so a `\"` inside a string does
/// not end it and a control byte outside a string (which is legal whitespace)
/// is preserved.
pub fn sanitize_control_chars(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut in_string = false;
    let mut escaped = false;
    for ch in raw.chars() {
        if in_string {
            if escaped {
                escaped = false;
                out.push(ch);
                continue;
            }
            match ch {
                '\\' => {
                    escaped = true;
                    out.push(ch);
                }
                '"' => {
                    in_string = false;
                    out.push(ch);
                }
                c if (c as u32) < 0x20 => out.push(' '),
                c => out.push(c),
            }
        } else {
            if ch == '"' {
                in_string = true;
            }
            out.push(ch);
        }
    }
    out
}

/// The primary top-level array of a manifest, by name.
///
/// Manifests are NOT single-key. `ExportWeapons_en.json` carries both
/// `ExportWeapons` (837 rows) and `ExportRailjackWeapons` (143), and serde's
/// map is ordered, so a "first array wins" heuristic silently reads the wrong
/// 143 rows — it did, until a live contract test caught it. Ask for the array
/// you mean.
///
/// Falls back to the LARGEST top-level array if the key is absent, so a rename
/// degrades to a plausible read instead of an empty one; a missing key is
/// still worth a warning, which the caller emits.
pub fn manifest_rows_named<'a>(doc: &'a Value, key: &str) -> &'a [Value] {
    let Some(obj) = doc.as_object() else { return &[] };
    if let Some(arr) = obj.get(key).and_then(|v| v.as_array()) {
        return arr.as_slice();
    }
    obj.values()
        .filter_map(|v| v.as_array())
        .max_by_key(|a| a.len())
        .map(|a| a.as_slice())
        .unwrap_or(&[])
}

/// The primary array for a manifest, derived from its basename.
///
/// `ExportWeapons_en.json` → key `ExportWeapons`. Handles `ExportManifest.json`
/// too, which has no `_en` infix.
pub fn manifest_rows_for<'a>(doc: &'a Value, basename: &str) -> &'a [Value] {
    let key = basename.trim_end_matches(".json").trim_end_matches("_en");
    manifest_rows_named(doc, key)
}

/// The largest top-level array, for callers that hold a manifest without
/// knowing its filename (tests, mostly).
pub fn manifest_rows(doc: &Value) -> &[Value] {
    doc.as_object()
        .and_then(|m| m.values().filter_map(|v| v.as_array()).max_by_key(|a| a.len()))
        .map(|a| a.as_slice())
        .unwrap_or(&[])
}

// ---------------------------------------------------------------------------
// Fetch stages
// ---------------------------------------------------------------------------

/// Everything one cycle pulls from DE. Any field may be absent — the caller
/// reconciles against the prior snapshot rather than failing the build.
#[derive(Debug, Default)]
pub struct DeSnapshot {
    /// basename → content hash, for manifests we can prove we hold. A manifest
    /// that failed to fetch or parse is ABSENT here even though the index
    /// named it — recording its hash would tell the next cycle we already have
    /// it and the failure would never be retried.
    pub hashes: BTreeMap<String, String>,
    /// Manifests actually fetched this cycle (skipped ones are absent).
    pub manifests: HashMap<String, Value>,
    /// Basenames whose hash moved since the prior cycle.
    pub changed: Vec<String>,
    /// Whether the index itself came back. False means DE is unreachable and
    /// the caller must fall back rather than carry — the distinction between
    /// "nothing changed" and "we could not look" is the whole safety of the
    /// skip.
    pub index_ok: bool,
    pub world: Option<Value>,
}

impl DeSnapshot {
    /// True when the manifest was deliberately skipped as unchanged — the
    /// caller should emit an empty surface and let `reconcile` carry the prior
    /// DE-derived one, NOT fall back to another source.
    pub fn skipped(&self, name: &str) -> bool {
        self.index_ok && !self.manifests.contains_key(name) && self.hashes.contains_key(name)
    }
}

/// Fetch the index and whichever wanted manifests moved since `prior_hashes`.
///
/// The 490-byte index is the whole point: a daily poll costs 490 bytes and
/// pulls only what actually changed. A cold run (`prior_hashes` empty) pulls
/// everything in [`WANTED_MANIFESTS`].
pub fn fetch_export(
    http: &dyn Http,
    prior_hashes: &BTreeMap<String, String>,
) -> DeSnapshot {
    let mut snap = DeSnapshot::default();

    let raw = match http.get_bytes(DE_INDEX_URL) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("  warning: could not fetch {DE_INDEX_URL}: {e}");
            return snap;
        }
    };
    let text = match decode_lzma_alone(&raw) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("  warning: {DE_INDEX_URL}: {e}");
            return snap;
        }
    };
    let index = parse_index(&text);
    if index.is_empty() {
        eprintln!("  warning: {DE_INDEX_URL}: index parsed to zero entries");
        return snap;
    }
    snap.index_ok = true;
    // Hashes for manifests we are not fetching at all pass through untouched —
    // they are provenance only. The wanted ones are recorded below, and only
    // once we actually hold them.
    snap.hashes = index
        .iter()
        .filter(|(k, _)| !WANTED_MANIFESTS.contains(&k.as_str()))
        .map(|(k, v)| (k.clone(), v.hash.clone()))
        .collect();

    for name in WANTED_MANIFESTS {
        let Some(entry) = index.get(*name) else {
            eprintln!("  warning: {name} is not in DE's index this cycle");
            continue;
        };
        let unchanged = prior_hashes.get(*name) == Some(&entry.hash);
        if unchanged && !ALWAYS_FETCH.contains(name) {
            // The skip that makes this cheap. Recording the hash marks it held,
            // which is what `skipped()` keys off.
            snap.hashes.insert((*name).to_string(), entry.hash.clone());
            continue;
        }
        if !unchanged {
            snap.changed.push((*name).to_string());
        }
        match http.get_text(&entry.url()) {
            Ok(body) => match parse_manifest(&body) {
                Ok(doc) => {
                    snap.manifests.insert((*name).to_string(), doc);
                    // Recorded ONLY on success. A hash written for a manifest
                    // we failed to read would tell the next cycle we already
                    // have it, and the failure would never be retried.
                    snap.hashes.insert((*name).to_string(), entry.hash.clone());
                }
                Err(e) => eprintln!("  warning: {name}: {e}"),
            },
            Err(e) => eprintln!("  warning: could not fetch {name}: {e}"),
        }
    }
    snap
}

/// Fetch worldState. Returns `None` on any failure — the caller keeps the
/// prior surface and marks it stale.
pub fn fetch_world_state(http: &dyn Http) -> Option<Value> {
    match http.get_json(DE_WORLD_STATE_URL) {
        Ok(v) if v.is_object() => Some(v),
        Ok(_) => {
            eprintln!("  warning: {DE_WORLD_STATE_URL}: not a JSON object");
            None
        }
        Err(e) => {
            eprintln!("  warning: could not fetch {DE_WORLD_STATE_URL}: {e}");
            None
        }
    }
}

// ---------------------------------------------------------------------------
// worldState helpers
// ---------------------------------------------------------------------------

/// Unwrap DE's Mongo-flavoured `{"$date":{"$numberLong":"1787317200000"}}`
/// into epoch millis.
///
/// Normalised at the edge so nothing downstream ever sees the wrapper. Also
/// accepts a bare number, which some worldState fields use.
pub fn de_millis(v: Option<&Value>) -> Option<i64> {
    let v = v?;
    if let Some(n) = v.as_i64() {
        return Some(n);
    }
    let inner = v.get("$date")?;
    if let Some(n) = inner.as_i64() {
        return Some(n);
    }
    let num = inner.get("$numberLong")?;
    num.as_i64().or_else(|| num.as_str().and_then(|s| s.parse().ok()))
}

/// Relay node id → the display name players use.
///
/// worldState says `PlutoHUB`; every human-facing surface says "Pluto Relay".
/// Unknown nodes fall through unchanged rather than being prettified into
/// something that might be wrong.
pub fn relay_name(node: &str) -> String {
    match node {
        "PlutoHUB" => "Pluto Relay".to_string(),
        "EarthHUB" => "Earth Relay".to_string(),
        "MercuryHUB" => "Mercury Relay".to_string(),
        "SaturnHUB" => "Saturn Relay".to_string(),
        "EuropaHUB" => "Europa Relay".to_string(),
        "ErisHUB" => "Eris Relay".to_string(),
        "VenusHUB" => "Venus Relay".to_string(),
        "TradeHUB1" => "Maroo's Bazaar".to_string(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_index_reads_file_and_hash() {
        let idx = parse_index(
            "ExportWeapons_en.json!00_abc\nExportRecipes_en.json!00_def\n",
        );
        assert_eq!(idx.len(), 2);
        assert_eq!(idx["ExportWeapons_en.json"].hash, "00_abc");
        assert_eq!(
            idx["ExportWeapons_en.json"].url(),
            "https://content.warframe.com/PublicExport/Manifest/ExportWeapons_en.json!00_abc"
        );
    }

    #[test]
    fn parse_index_skips_hashless_lines() {
        // A hashless URL 404s, so a line we cannot hash is dropped, not guessed.
        let idx = parse_index("ExportWeapons_en.json\n\nExportRecipes_en.json!00_def\n");
        assert_eq!(idx.len(), 1);
        assert!(idx.contains_key("ExportRecipes_en.json"));
    }

    #[test]
    fn sanitizer_only_touches_control_chars_inside_strings() {
        let raw = "{\n  \"a\": \"we\u{0007}ird\",\n  \"b\": 1\n}";
        let cleaned = sanitize_control_chars(raw);
        assert!(cleaned.contains("\"we ird\""));
        // Structural newlines survive.
        assert!(cleaned.contains("\n  \"b\": 1"));
        let v: Value = serde_json::from_str(&cleaned).unwrap();
        assert_eq!(v["a"], "we ird");
    }

    #[test]
    fn sanitizer_respects_escaped_quotes() {
        let raw = r#"{"a":"say \"hi\"","b":2}"#;
        assert_eq!(sanitize_control_chars(raw), raw);
    }

    #[test]
    fn parse_manifest_prefers_strict() {
        let v = parse_manifest(r#"{"ExportWeapons":[{"name":"Braton"}]}"#).unwrap();
        assert_eq!(manifest_rows(&v).len(), 1);
    }

    #[test]
    fn parse_manifest_falls_back_for_control_chars() {
        let v = parse_manifest("{\"ExportWeapons\":[{\"description\":\"a\u{0001}b\"}]}").unwrap();
        assert_eq!(manifest_rows(&v)[0]["description"], "a b");
    }

    #[test]
    fn manifest_rows_named_picks_the_array_you_asked_for() {
        // The real ExportWeapons_en.json shape: two top-level arrays, and the
        // decoy sorts FIRST. Reading "the first array" silently returned the
        // 143 railjack rows instead of the 837 weapons — caught in production
        // only by the live contract test.
        let v: Value = serde_json::from_str(
            r#"{"ExportRailjackWeapons":[1,2,3],"ExportWeapons":[1,2,3,4,5]}"#,
        )
        .unwrap();
        assert_eq!(manifest_rows_named(&v, "ExportWeapons").len(), 5);
        assert_eq!(manifest_rows_named(&v, "ExportRailjackWeapons").len(), 3);
    }

    #[test]
    fn manifest_rows_for_derives_the_key_from_the_filename() {
        let v: Value = serde_json::from_str(
            r#"{"ExportRailjackWeapons":[1,2,3],"ExportWeapons":[1,2,3,4,5]}"#,
        )
        .unwrap();
        assert_eq!(manifest_rows_for(&v, "ExportWeapons_en.json").len(), 5);

        let m: Value = serde_json::from_str(r#"{"Manifest":[1,2]}"#).unwrap();
        assert_eq!(manifest_rows_for(&m, "ExportManifest.json").len(), 2);
    }

    #[test]
    fn manifest_rows_named_falls_back_to_the_largest_array_on_a_rename() {
        let v: Value = serde_json::from_str(r#"{"SomethingDeRenamed":[1,2,3],"Tiny":[1]}"#).unwrap();
        assert_eq!(manifest_rows_named(&v, "ExportWeapons").len(), 3);
    }

    #[test]
    fn de_millis_unwraps_every_shape() {
        let wrapped: Value =
            serde_json::from_str(r#"{"Activation":{"$date":{"$numberLong":"1787317200000"}}}"#)
                .unwrap();
        assert_eq!(de_millis(wrapped.get("Activation")), Some(1787317200000));

        let bare: Value = serde_json::from_str(r#"{"Activation":1787317200000}"#).unwrap();
        assert_eq!(de_millis(bare.get("Activation")), Some(1787317200000));

        assert_eq!(de_millis(None), None);
    }

    #[test]
    fn usage_url_builds_the_year_file() {
        assert_eq!(
            usage_url(2025),
            "https://www-static.warframe.com/repos/WarframeUsageData2025.json"
        );
    }

    #[test]
    fn relay_name_passes_unknown_nodes_through() {
        assert_eq!(relay_name("PlutoHUB"), "Pluto Relay");
        assert_eq!(relay_name("SomeNewHUB"), "SomeNewHUB");
    }

    #[test]
    fn lzma_alone_round_trips_a_real_stream() {
        // Byte-for-byte the alone-format container DE serves: 5-byte props,
        // 8-byte unknown size, then the stream. Produced by python's
        // lzma.compress(b"ExportWeapons_en.json!00_abc\n", format=FORMAT_ALONE).
        let b64 = "XQAAgAD//////////wAingoHEY9IBeul/igQedVd/DS3zFnbU+I+EEtzpT/OZLg//9DKEAA=";
        use base64::Engine as _;
        let bytes = base64::engine::general_purpose::STANDARD.decode(b64).unwrap();
        let text = decode_lzma_alone(&bytes).unwrap();
        assert_eq!(text.trim(), "ExportWeapons_en.json!00_abc");
    }
}
