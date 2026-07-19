// TennoWorth desktop shell — Phase C day-1 spike scaffold.
//
// Default behaviour (no env): a single window that loads the built SPA
// (prototype/dist) over Tauri's asset protocol, plus the app-defined `hello`
// command proving a wfm-core IPC round-trip. That is the whole minimal shell.
//
// Spike instrumentation is opt-in via SPIKE_PROBE=1, which injects a
// document-start probe (PROBE_JS) that records origin / storage / fetch / IPC
// behaviour and stashes it in localStorage + IndexedDB (CSP-independent) and,
// if IPC is reachable, ships it to `spike_report`. Kept behind the env so the
// committed shell stays a plain window.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::io::Write;
use tauri::{WebviewUrl, WebviewWindowBuilder};

/// Trivial IPC round-trip: returns the linked wfm-core version, proving the
/// SPA can reach a real wfm-core through a Tauri command (the C2 premise).
#[tauri::command]
fn hello() -> String {
    format!("wfm-core {}", wfm_core::version())
}

/// Spike-only: persist the probe's evidence JSON to $SPIKE_OUT (and echo it to
/// stdout between markers so it is captured even without file access).
#[tauri::command]
fn spike_report(payload: String) -> Result<String, String> {
    let out = std::env::var("SPIKE_OUT").unwrap_or_else(|_| "/tmp/spike-report.json".into());
    std::fs::write(&out, payload.as_bytes()).map_err(|e| e.to_string())?;
    let mut so = std::io::stdout();
    let _ = writeln!(so, "SPIKE_REPORT_FILE {out}");
    let _ = writeln!(so, "SPIKE_REPORT_BEGIN");
    let _ = writeln!(so, "{payload}");
    let _ = writeln!(so, "SPIKE_REPORT_END");
    let _ = so.flush();
    Ok(out)
}

/// Spike-only: let the probe close the app so the restart-persistence test can
/// run two clean launches without a human closing the window.
#[tauri::command]
fn spike_exit() {
    let mut so = std::io::stdout();
    let _ = writeln!(so, "SPIKE_EXIT");
    let _ = so.flush();
    std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_millis(300));
        std::process::exit(0);
    });
}

const PROBE_JS: &str = r#"(function(){
  var R = { runtag: "__RUNTAG__", steps_ts: new Date().toISOString(), cspViolations: [], consoleErrors: [] };
  // Register at document-start so we catch the SPA's own boot, not just our probe.
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
  function idbOpen(){ return new Promise(function(res,rej){ var q=indexedDB.open('spikeDB',1); q.onupgradeneeded=function(){ q.result.createObjectStore('kv'); }; q.onsuccess=function(){ res(q.result); }; q.onerror=function(){ rej(q.error); }; }); }
  function idbPut(k,v){ return idbOpen().then(function(db){ return new Promise(function(res,rej){ var t=db.transaction('kv','readwrite'); t.objectStore('kv').put(v,k); t.oncomplete=function(){ res(true); }; t.onerror=function(){ rej(t.error); }; }); }); }
  function idbGet(k){ return idbOpen().then(function(db){ return new Promise(function(res,rej){ var t=db.transaction('kv','readonly'); var g=t.objectStore('kv').get(k); g.onsuccess=function(){ res(g.result===undefined?null:g.result); }; g.onerror=function(){ rej(g.error); }; }); }); }
  function probeFetch(url){
    return fetch(url, { cache:'no-store' }).then(function(r){
      return r.text().then(function(b){ return { ok:r.ok, status:r.status, type:r.type, len:b.length, head:b.slice(0,48) }; });
    }).catch(function(e){ return { error: String(e && e.message || e), name: e && e.name }; });
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
    } catch(e){ R.envErr = String(e); }
    var marker = R.runtag + '@' + new Date().toISOString();
    try { R.priorLocalStorage = localStorage.getItem('__spike_marker__'); } catch(e){ R.priorLocalStorage = 'ERR:'+e; }
    idbGet('marker').then(function(v){ R.priorIndexedDB = v; }, function(e){ R.priorIndexedDB = 'ERR:'+e; })
    .then(function(){ try { localStorage.setItem('__spike_marker__', marker); R.wroteLocalStorage = marker; } catch(e){ R.wroteLocalStorage='ERR:'+e; } })
    .then(function(){ return idbPut('marker', marker).then(function(){ R.wroteIndexedDB = marker; }, function(e){ R.wroteIndexedDB='ERR:'+e; }); })
    .then(function(){ return probeFetch('/market.json').then(function(x){ R.fetchMarket = x; }); })
    .then(function(){ return probeFetch('/wfstat-catalog.json').then(function(x){ R.fetchCatalog = x; }); })
    .then(function(){ return probeFetch('https://tennoworth.app/market.json').then(function(x){ R.fetchRemote = x; }); })
    .then(function(){
      var inv = invokeFn();
      if (!inv) { R.invokeHello = 'NO_INVOKE_FN'; return; }
      return inv('hello').then(function(v){ R.invokeHello = v; }, function(e){ R.invokeHello = 'ERR:'+(e && e.message || e); });
    })
    .then(function(){
      R.done = true;
      var json = JSON.stringify(R);
      try { localStorage.setItem('__spike_report__', json); } catch(e){}
      return idbPut('__spike_report__', json).catch(function(){}).then(function(){
        try { document.title = 'SPIKE_DONE ' + R.runtag; } catch(e){}
        var inv = invokeFn();
        if (inv) {
          inv('spike_report', { payload: json }).catch(function(){}).then(function(){
            setTimeout(function(){ inv('spike_exit').catch(function(){}); }, 400);
          });
        }
      });
    })
    .catch(function(e){ try { R.fatal='ERR:'+(e && e.message || e); localStorage.setItem('__spike_report__', JSON.stringify(R)); } catch(_){} });
  }
  if (document.readyState === 'complete') setTimeout(run, 900);
  else window.addEventListener('load', function(){ setTimeout(run, 900); });
})();"#;

fn main() {
    let probe = std::env::var("SPIKE_PROBE").ok().as_deref() == Some("1");
    let runtag = std::env::var("SPIKE_RUNTAG").unwrap_or_else(|_| "na".into());

    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![hello, spike_report, spike_exit])
        .setup(move |app| {
            let mut b = WebviewWindowBuilder::new(app, "main", WebviewUrl::default())
                .title("TennoWorth")
                .inner_size(1200.0, 800.0);
            if probe {
                b = b.initialization_script(&PROBE_JS.replace("__RUNTAG__", &runtag));
            }
            let w = b.build()?;
            let mut so = std::io::stdout();
            match w.url() {
                Ok(u) => {
                    let _ = writeln!(so, "SPIKE_WEBVIEW_URL {u}");
                }
                Err(e) => {
                    let _ = writeln!(so, "SPIKE_WEBVIEW_URL_ERR {e}");
                }
            }
            let _ = writeln!(so, "SPIKE_PROBE_ENABLED {probe}");
            let _ = so.flush();
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
