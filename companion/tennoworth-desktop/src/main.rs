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

use std::io::Write;
use std::sync::OnceLock;
use tauri::{WebviewUrl, WebviewWindowBuilder};

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

/// Memory-scan the running game and return the inventory JSON as a string —
/// the exact bytes the CLI would write to inventory.json. Async + spawn_blocking
/// so the (potentially slow) scan never blocks the webview event loop. A busy
/// guard or a missing/unscannable game becomes a rejected invoke carrying
/// wfm-core's graceful, actionable message (e.g. "Warframe doesn't appear to be
/// running…") — the SPA surfaces it verbatim in its error banner.
#[tauri::command]
async fn scan_inventory() -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(|| match scanner().scan(None, None) {
        Ok((bytes, _info)) => String::from_utf8(bytes)
            .map_err(|e| format!("inventory response was not valid UTF-8: {e}")),
        Err(e) => Err(e.into_message()),
    })
    .await
    .map_err(|e| format!("scan task failed to run: {e}"))?
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

const PROBE_JS: &str = r#"(function(){
  var R = { runtag: "__RUNTAG__", steps_ts: new Date().toISOString(), cspViolations: [], consoleErrors: [] };
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
  function probeFetch(url){
    return fetch(url, { cache:'no-store' }).then(function(r){
      return r.text().then(function(b){ return { ok:r.ok, status:r.status, type:r.type, len:b.length, head:b.slice(0,48) }; });
    }).catch(function(e){ return { error: String(e && e.message || e), name: e && e.name }; });
  }
  function delay(ms){ return new Promise(function(res){ setTimeout(res, ms); }); }
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
    // Persistence marker chain: read the prior run's, write this run's.
    var marker = R.runtag + '@' + new Date().toISOString();
    try { R.priorMarker = localStorage.getItem('__tennoworth_probe_marker__'); } catch(e){ R.priorMarker = 'ERR:'+e; }
    try { localStorage.setItem('__tennoworth_probe_marker__', marker); R.wroteMarker = marker; } catch(e){ R.wroteMarker='ERR:'+e; }
    probeFetch('/market.json')
    .then(function(x){ R.fetchMarket = x; })
    .then(function(){ return probeFetch('/wfstat-catalog.json').then(function(x){ R.fetchCatalog = x; }); })
    .then(function(){
      var inv = invokeFn();
      if (!inv) { R.invokeHealth = 'NO_INVOKE_FN'; return; }
      return inv('health').then(function(v){ R.invokeHealth = v; }, function(e){ R.invokeHealth = 'ERR:'+(e && e.message || e); });
    })
    .then(function(){
      // Drive the REAL scan button and read the resulting UI banner — proves the
      // full desktop scan flow (button → transport → scan_inventory → banner).
      var btn = document.querySelector('[data-testid="desktop-scan"]');
      R.scanButtonFound = !!btn;
      if (!btn) return;
      btn.click();
      return delay(1500).then(function(){
        var banner = document.querySelector('.general-banner .gb-body');
        R.scanBannerText = banner ? (banner.innerText || '').slice(0, 300) : null;
      });
    })
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
    .catch(function(e){ try { R.fatal='ERR:'+(e && e.message || e); localStorage.setItem('__tennoworth_probe_report__', JSON.stringify(R)); } catch(_){} });
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
            probe_report,
            probe_exit
        ])
        .setup(move |app| {
            let mut b = WebviewWindowBuilder::new(app, "main", WebviewUrl::default())
                .title("TennoWorth")
                .inner_size(1200.0, 800.0);
            if probe {
                b = b.initialization_script(&PROBE_JS.replace("__RUNTAG__", &runtag));
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
