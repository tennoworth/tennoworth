// TennoWorth desktop shell (Tauri v2). The webview loads the built SPA
// (prototype/dist) over Tauri's asset protocol; the SPA's Transport picks the
// Tauri path at boot and drives wfm-core through these commands instead of the
// loopback HTTP companion.
//
// Commands are deliberately thin adapters over wfm-core (the CLI is the other
// adapter over the same crate):
//   - `health`         → version / platform info (the IPC liveness round-trip)
//   - `scan_inventory` → single-flight memory scan → inventory JSON bytes
//
// The login / listing / assistant surface (which needs the passphrase UI) is
// NOT wired here yet — the SPA hides those affordances in desktop mode.
//
// A verification probe is opt-in behind TENNOWORTH_PROBE=1: it injects a
// document-start script (PROBE_JS) that records origin / storage / fetch / IPC /
// CSP-violation behaviour, drives the real scan button, and exfiltrates the
// evidence via `probe_report` (→ file + stdout) before auto-exiting. Kept behind
// the env so the default run is a plain window.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod db;
mod market;
mod sellables;
mod snapshot;

use std::io::Write;
use std::sync::OnceLock;
use tauri::{Manager, State, WebviewUrl, WebviewWindowBuilder};

use db::{Db, Reserve, SnapshotSummary};
use market::{MarketCache, RefreshResult};
use wfm_core::inventory::InventoryScanner;

/// Process-wide scanner so the single-flight guard actually serializes two
/// concurrent `scan_inventory` invokes (a second concurrent scan gets
/// ScanError::Busy rather than a redundant parallel walk of the address space).
fn scanner() -> &'static InventoryScanner {
    static SCANNER: OnceLock<InventoryScanner> = OnceLock::new();
    SCANNER.get_or_init(InventoryScanner::new)
}

#[derive(serde::Serialize)]
struct Health {
    ok: bool,
    /// OS family the shell was built for (`linux` / `windows` / `macos`).
    platform: String,
    app_version: String,
    core_version: String,
}

/// IPC liveness + build info. Proves the SPA can reach a live wfm-core over the
/// Tauri boundary (the desktop analogue of the loopback `/health` probe).
#[tauri::command]
fn health() -> Health {
    Health {
        ok: true,
        platform: std::env::consts::OS.to_string(),
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        core_version: wfm_core::version().to_string(),
    }
}

/// Extract snapshot rows from raw inventory bytes and append them to history as
/// one transactional snapshot. Shared by the memory scan and the file-drop
/// import. Returns the new snapshot id.
fn record_snapshot(
    db: &Db,
    source: &str,
    game_version: Option<&str>,
    bytes: &[u8],
) -> Result<i64, String> {
    let items = snapshot::extract_items(bytes)
        .map_err(|e| format!("parse inventory for snapshot: {e}"))?;
    db.insert_snapshot(source, None, game_version, &items)
        .map_err(|e| format!("insert snapshot: {e}"))
}

/// Memory-scan the running game and return the inventory JSON as a string —
/// the exact bytes the CLI would write to inventory.json. Async + spawn_blocking
/// so the (potentially slow) scan never blocks the webview event loop. A busy
/// guard or a missing/unscannable game becomes a rejected invoke carrying
/// wfm-core's graceful, actionable message (e.g. "Warframe doesn't appear to be
/// running…") — the SPA surfaces it verbatim in its error banner.
///
/// On success it also appends a `source='memory'` history snapshot. That insert
/// is best-effort: a failure is logged to stderr and swallowed — losing a
/// history row must never cost the user their scan (scan value > history value).
#[tauri::command]
async fn scan_inventory(db: State<'_, Db>) -> Result<String, String> {
    let (bytes, info) = tauri::async_runtime::spawn_blocking(|| scanner().scan(None, None))
        .await
        .map_err(|e| format!("scan task failed to run: {e}"))?
        .map_err(|e| e.into_message())?;

    if let Err(e) = record_snapshot(&db, "memory", info.build.as_deref(), &bytes) {
        eprintln!("tennoworth: inventory snapshot not recorded: {e}");
    }

    String::from_utf8(bytes).map_err(|e| format!("inventory response was not valid UTF-8: {e}"))
}

/// Record a dropped inventory.json as a `source='import'` history snapshot.
/// Used by the desktop file-drop path; unlike the scan path this surfaces the
/// error to the caller (the SPA logs it and moves on).
#[tauri::command]
fn import_snapshot(db: State<'_, Db>, inventory_json: String) -> Result<i64, String> {
    record_snapshot(&db, "import", None, inventory_json.as_bytes())
}

#[tauri::command]
fn get_setting(db: State<'_, Db>, key: String) -> Result<Option<String>, String> {
    db.get_setting(&key).map_err(|e| e.to_string())
}

#[tauri::command]
fn set_setting(db: State<'_, Db>, key: String, value: String) -> Result<(), String> {
    db.set_setting(&key, &value).map_err(|e| e.to_string())
}

#[tauri::command]
fn get_reserves(db: State<'_, Db>) -> Result<Vec<Reserve>, String> {
    db.get_reserves().map_err(|e| e.to_string())
}

#[tauri::command]
fn set_reserve(db: State<'_, Db>, slug: String, keep: i64) -> Result<(), String> {
    db.set_reserve(&slug, keep).map_err(|e| e.to_string())
}

#[tauri::command]
fn delete_reserve(db: State<'_, Db>, slug: String) -> Result<(), String> {
    db.delete_reserve(&slug).map_err(|e| e.to_string())
}

#[tauri::command]
fn list_snapshots(db: State<'_, Db>, limit: i64) -> Result<Vec<SnapshotSummary>, String> {
    db.list_snapshots(limit).map_err(|e| e.to_string())
}

/// The app-data-cached market snapshot, or null on a first run / unreadable
/// cache. No network: the SPA reads this at boot to prefer the cache (last
/// known-good from the live server) over the compile-time bundled floor.
#[tauri::command]
fn cached_market(cache: State<'_, MarketCache>) -> Option<String> {
    cache.cached()
}

/// Conditionally refresh the market snapshot from tennoworth.app (ETag /
/// If-None-Match), updating the app-data cache. Async + spawn_blocking so the
/// (network) call never blocks the webview event loop, mirroring scan_inventory
/// (reqwest::blocking must not run on an async worker thread). Every network /
/// HTTP / body failure is swallowed inside `market::refresh` and returns a
/// no-op RefreshResult — the only Err here is the blocking task failing to run.
#[tauri::command]
async fn refresh_market(cache: State<'_, MarketCache>) -> Result<RefreshResult, String> {
    let dir = cache.dir();
    tauri::async_runtime::spawn_blocking(move || market::refresh(&dir))
        .await
        .map_err(|e| format!("market refresh task failed to run: {e}"))
}

/// Rank the latest snapshot × market by the shared sell-priority score and
/// return the top `limit` sellables. The single join both the tray menu and the
/// post-scan notification consume; also available to the SPA. Reads the freshest
/// market it holds (app-data cache, else the compile-time bundle).
#[tauri::command]
fn top_sellables(
    db: State<'_, Db>,
    cache: State<'_, MarketCache>,
    limit: usize,
) -> Vec<sellables::SellableRow> {
    let market = sellables::MarketData::load(&cache);
    let mut rows = sellables::rank_sellables(&db, &market);
    rows.truncate(limit);
    rows
}

/// Probe-only: persist the evidence JSON to $TENNOWORTH_PROBE_OUT (and echo it
/// to stdout between markers so it is captured even without file access).
#[tauri::command]
fn probe_report(payload: String) -> Result<String, String> {
    let out = std::env::var("TENNOWORTH_PROBE_OUT")
        .unwrap_or_else(|_| "/tmp/tennoworth-probe.json".into());
    std::fs::write(&out, payload.as_bytes()).map_err(|e| e.to_string())?;
    let mut so = std::io::stdout();
    let _ = writeln!(so, "PROBE_REPORT_FILE {out}");
    let _ = writeln!(so, "PROBE_REPORT_BEGIN");
    let _ = writeln!(so, "{payload}");
    let _ = writeln!(so, "PROBE_REPORT_END");
    let _ = so.flush();
    Ok(out)
}

/// Probe-only: close the app so the restart-persistence check can run two clean
/// launches without a human closing the window.
#[tauri::command]
fn probe_exit() {
    let mut so = std::io::stdout();
    let _ = writeln!(so, "PROBE_EXIT");
    let _ = so.flush();
    std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_millis(300));
        std::process::exit(0);
    });
}

// A minimal DE inventory: four distinct tradeable-category paths (so
// extract_items → item_count == 4), three of which resolve to WFM slugs with
// market stats (so the drop flips to the sell view and the reserve input
// renders). Kept in sync with the categories snapshot::extract_items walks.
const PROBE_FIXTURE: &str = r#"{
  "RawUpgrades": [
    { "ItemType": "/Lotus/Upgrades/Mods/Shotgun/DualStat/AcceleratedBlastMod", "ItemCount": 3 },
    { "ItemType": "/Lotus/Powersuits/Trinity/LinkAugmentCard", "ItemCount": 2 },
    { "ItemType": "/Lotus/Powersuits/Khora/KhoraCrackAugmentCard", "ItemCount": 5 }
  ],
  "MiscItems": [
    { "ItemType": "/Lotus/Types/Items/MiscItems/OrokinCell", "ItemCount": 42 }
  ]
}"#;

const PROBE_JS: &str = r#"(function(){
  var R = { runtag: "__RUNTAG__", steps_ts: new Date().toISOString(), cspViolations: [], consoleErrors: [] };
  var FIXTURE = __FIXTURE__;
  try {
    document.addEventListener('securitypolicyviolation', function(e){
      if (R.cspViolations.length < 20) R.cspViolations.push({ blockedURI: e.blockedURI, violatedDirective: e.violatedDirective, effectiveDirective: e.effectiveDirective, disposition: e.disposition });
    });
  } catch(e){}
  try {
    var origErr = console.error.bind(console);
    console.error = function(){ try { if (R.consoleErrors.length < 20) R.consoleErrors.push(Array.prototype.slice.call(arguments).map(String).join(' ').slice(0,200)); } catch(_){} return origErr.apply(null, arguments); };
  } catch(e){}
  function invokeFn(){
    try {
      if (window.__TAURI__ && window.__TAURI__.core && window.__TAURI__.core.invoke) return window.__TAURI__.core.invoke;
      if (window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke) return window.__TAURI_INTERNALS__.invoke;
    } catch(e){}
    return null;
  }
  function invk(cmd, args){
    var inv = invokeFn();
    if (!inv) return Promise.resolve('NO_INVOKE_FN');
    return inv(cmd, args).catch(function(e){ return 'ERR:'+(e && e.message || e); });
  }
  function probeFetch(url){
    return fetch(url, { cache:'no-store' }).then(function(r){
      return r.text().then(function(b){ return { ok:r.ok, status:r.status, type:r.type, len:b.length, head:b.slice(0,48) }; });
    }).catch(function(e){ return { error: String(e && e.message || e), name: e && e.name }; });
  }
  function delay(ms){ return new Promise(function(res){ setTimeout(res, ms); }); }
  // Synthesize a REAL file-drop onto the DropZone: webkit's DragEvent ctor won't
  // carry a dataTransfer, so attach one with a File via defineProperty. This
  // exercises DropZone → handleInventory(origin:'file') → import_snapshot.
  function dropFixture(){
    try {
      var dz = document.querySelector('.dropzone');
      if (!dz) return 'NO_DROPZONE';
      var file = new File([FIXTURE], 'fixture-inventory.json', { type: 'application/json' });
      var dt = new DataTransfer();
      dt.items.add(file);
      var ev = new Event('drop', { bubbles: true, cancelable: true });
      Object.defineProperty(ev, 'dataTransfer', { value: dt });
      dz.dispatchEvent(ev);
      return 'DROPPED';
    } catch(e){ return 'ERR:'+(e && e.message || e); }
  }
  // Drive the REAL reserve input if the sell view rendered (→ setReserveCopies →
  // store.setSetting → set_setting); else fall back to the raw command so the
  // scenario still records evidence. Reports which path was taken.
  function setReserve(){
    try {
      var el = document.querySelector('[data-testid="reserve-copies"]');
      if (el) {
        var setter = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, 'value').set;
        setter.call(el, '4');
        el.dispatchEvent(new Event('input', { bubbles: true }));
        return delay(300).then(function(){ return 'UI'; });
      }
    } catch(e){ return Promise.resolve('UI_ERR:'+(e && e.message || e)); }
    return invk('set_setting', { key: 'reserve-copies', value: '4' }).then(function(){ return 'INVOKE'; });
  }
  function run(){
    try {
      R.origin = location.origin;
      R.href = location.href;
      R.protocol = location.protocol;
      R.hasGlobalTauri = typeof window.__TAURI__ !== 'undefined';
      R.hasInternals = typeof window.__TAURI_INTERNALS__ !== 'undefined';
      R.spaTitle = document.title;
      var app = document.querySelector('#app');
      R.appMounted = !!(app && app.childElementCount > 0);
      R.appChildCount = app ? app.childElementCount : -1;
      R.bodyTextLen = (document.body && document.body.innerText || '').length;
      R.desktopBadge = !!document.querySelector('[data-testid="desktop-mode"]');
      R.marketBrowserRendered = !!document.querySelector('.market-browser, [data-testid="market-browser"]');
    } catch(e){ R.envErr = String(e); }
    // Persistence marker chain (webview localStorage — separate from the SQLite
    // store; the real cross-restart proof is reserveAtStart via get_setting).
    var marker = R.runtag + '@' + new Date().toISOString();
    try { R.priorMarker = localStorage.getItem('__tennoworth_probe_marker__'); } catch(e){ R.priorMarker = 'ERR:'+e; }
    try { localStorage.setItem('__tennoworth_probe_marker__', marker); R.wroteMarker = marker; } catch(e){ R.wroteMarker='ERR:'+e; }
    probeFetch('/market.json')
    .then(function(x){ R.fetchMarket = x; })
    .then(function(){ return probeFetch('/wfstat-catalog.json').then(function(x){ R.fetchCatalog = x; }); })
    .then(function(){ return invk('health').then(function(v){ R.invokeHealth = v; }); })
    // C4 market refresh: cache presence before, the conditional-GET outcome, then
    // cache presence after. Across launches this shows 200 → cache written, then
    // If-None-Match → 304 → cache kept (updated:false, no body re-sent).
    .then(function(){ return invk('cached_market').then(function(v){ R.cachedMarketBefore = (typeof v === 'string' && v.length > 0); R.cachedMarketBeforeLen = (typeof v === 'string') ? v.length : 0; }); })
    .then(function(){ return invk('refresh_market').then(function(v){ R.marketRefresh = (v && typeof v === 'object') ? { updated: v.updated, updated_at: v.updated_at, etag: v.etag, bodyLen: v.body ? v.body.length : 0 } : v; }); })
    .then(function(){ return invk('cached_market').then(function(v){ R.cachedMarketAfter = (typeof v === 'string' && v.length > 0); R.cachedMarketAfterLen = (typeof v === 'string') ? v.length : 0; }); })
    // Cross-restart persistence: the value a PRIOR run wrote. null on run 1,
    // '4' on run 2 → proves the SQLite setting survived the restart.
    .then(function(){ return invk('get_setting', { key: 'reserve-copies' }).then(function(v){ R.reserveAtStart = v; }); })
    .then(function(){ return invk('list_snapshots', { limit: 50 }).then(function(v){ R.snapshotsAtStart = v; }); })
    // (b) Scan with no game running: drive the REAL scan button. Graceful error
    // banner, and — critically — NO source='memory' snapshot row is added.
    .then(function(){
      var btn = document.querySelector('[data-testid="desktop-scan"]');
      R.scanButtonFound = !!btn;
      if (!btn) return;
      btn.click();
      return delay(1800).then(function(){
        var banner = document.querySelector('.general-banner .gb-body');
        R.scanBannerText = banner ? (banner.innerText || '').slice(0, 300) : null;
      });
    })
    .then(function(){ return invk('list_snapshots', { limit: 50 }).then(function(v){ R.snapshotsAfterScan = v; }); })
    // (c) File-drop the fixture → source='import' snapshot with item_count == 4.
    .then(function(){ R.dropResult = dropFixture(); return delay(2500); })
    .then(function(){
      R.phaseDoneAfterDrop = !document.querySelector('[data-testid="desktop-mode"] .dropzone, .dropzone');
      R.resultsRowsAfterDrop = document.querySelectorAll('table tbody tr').length;
      return invk('list_snapshots', { limit: 50 }).then(function(v){ R.snapshotsAfterDrop = v; });
    })
    // (a) Set reserve via the REAL input (now rendered) → set_setting.
    .then(function(){ return setReserve().then(function(via){ R.reserveSetVia = via; }); })
    .then(function(){ return invk('get_setting', { key: 'reserve-copies' }).then(function(v){ R.reserveAfterSet = v; }); })
    .then(function(){
      R.done = true;
      var json = JSON.stringify(R);
      try { localStorage.setItem('__tennoworth_probe_report__', json); } catch(e){}
      try { document.title = 'PROBE_DONE ' + R.runtag; } catch(e){}
      var inv = invokeFn();
      if (inv) {
        inv('probe_report', { payload: json }).catch(function(){}).then(function(){
          setTimeout(function(){ inv('probe_exit').catch(function(){}); }, 400);
        });
      }
    })
    .catch(function(e){ try { R.fatal='ERR:'+(e && e.message || e); localStorage.setItem('__tennoworth_probe_report__', JSON.stringify(R)); invk('probe_report', { payload: JSON.stringify(R) }); } catch(_){} });
  }
  if (document.readyState === 'complete') setTimeout(run, 900);
  else window.addEventListener('load', function(){ setTimeout(run, 900); });
})();"#;

fn main() {
    let probe = std::env::var("TENNOWORTH_PROBE").ok().as_deref() == Some("1");
    let runtag = std::env::var("TENNOWORTH_RUNTAG").unwrap_or_else(|_| "na".into());

    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            health,
            scan_inventory,
            import_snapshot,
            get_setting,
            set_setting,
            get_reserves,
            set_reserve,
            delete_reserve,
            list_snapshots,
            cached_market,
            refresh_market,
            top_sellables,
            probe_report,
            probe_exit
        ])
        .setup(move |app| {
            // Open the canonical SQLite store in the platform app-data dir and
            // hand it to the command layer as managed state. A failure here is
            // unrecoverable (the store is canonical) — abort startup with a
            // clear message rather than run with silent, ephemeral state.
            let data_dir = app
                .path()
                .app_data_dir()
                .map_err(|e| format!("resolving app data dir: {e}"))?;
            std::fs::create_dir_all(&data_dir)
                .map_err(|e| format!("creating app data dir {}: {e}", data_dir.display()))?;
            let db_path = data_dir.join("tennoworth.db");
            let store = Db::open(&db_path)
                .map_err(|e| format!("opening state DB {}: {e}", db_path.display()))?;
            app.manage(store);

            // The C4 market cache lives next to the DB in the same app-data dir.
            // Unlike the DB, a missing/unreadable cache is never fatal — the
            // bundled snapshot is the floor — so this can't fail startup.
            app.manage(MarketCache::new(data_dir));

            let mut b = WebviewWindowBuilder::new(app, "main", WebviewUrl::default())
                .title("TennoWorth")
                .inner_size(1200.0, 800.0);
            if probe {
                // serde_json turns the fixture &str into a quoted, escaped JS
                // string literal so `var FIXTURE = __FIXTURE__;` parses.
                let fixture_literal = serde_json::to_string(PROBE_FIXTURE)
                    .expect("probe fixture serializes to a JS string literal");
                let js = PROBE_JS
                    .replace("__RUNTAG__", &runtag)
                    .replace("__FIXTURE__", &fixture_literal);
                b = b.initialization_script(&js);
            }
            let w = b.build()?;
            if probe {
                let mut so = std::io::stdout();
                match w.url() {
                    Ok(u) => {
                        let _ = writeln!(so, "PROBE_WEBVIEW_URL {u}");
                    }
                    Err(e) => {
                        let _ = writeln!(so, "PROBE_WEBVIEW_URL_ERR {e}");
                    }
                }
                let _ = writeln!(so, "PROBE_ENABLED true");
                let _ = so.flush();
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
