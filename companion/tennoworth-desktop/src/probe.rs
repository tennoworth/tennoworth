//! Verification probe, opt-in behind `TENNOWORTH_PROBE=1`: a document-start
//! script (`PROBE_JS`) that records origin / storage / fetch / IPC /
//! CSP-violation behaviour, drives the real scan + listing UI against a
//! synthetic fixture, and exfiltrates the evidence via `probe_report`
//! (file + stdout) before auto-exiting. The probe-only commands here all
//! refuse to run unless `TENNOWORTH_PROBE=1`, so a normal build can't reach
//! them.

use std::collections::BTreeMap;
use std::io::Write;
use std::sync::Arc;
use tauri::{AppHandle, Manager, State};

use wfm_core::listing::Unlocked;
use wfm_core::poison::guard;

use crate::sellables::ScanNotification;
use crate::tray::{post_scan_surfaces, TrayState};
use crate::wfm_session::{CmdError, WfmSession};

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
  // Like invk, but keeps the typed CmdError shape: rejections come back as
  // { ok:false, code, message } so the report can assert needs_login vs
  // needs_unlock vs bad_passphrase instead of a flattened string.
  function invkE(cmd, args){
    var inv = invokeFn();
    if (!inv) return Promise.resolve('NO_INVOKE_FN');
    return inv(cmd, args).then(
      function(v){ return { ok: true, value: v === undefined ? null : v }; },
      function(e){ return { ok: false, code: e && e.code, message: String(e && e.message || e).slice(0, 160) }; }
    );
  }
  function probeFetch(url){
    return fetch(url, { cache:'no-store' }).then(function(r){
      return r.text().then(function(b){ return { ok:r.ok, status:r.status, type:r.type, len:b.length, head:b.slice(0,48) }; });
    }).catch(function(e){ return { error: String(e && e.message || e), name: e && e.name }; });
  }
  function delay(ms){ return new Promise(function(res){ setTimeout(res, ms); }); }
  function curWin(){
    try { if (window.__TAURI__ && window.__TAURI__.window && window.__TAURI__.window.getCurrentWindow) return window.__TAURI__.window.getCurrentWindow(); } catch(e){}
    return null;
  }
  // C6 lifecycle: window.close() fires CloseRequested, which Rust intercepts
  // (prevent_close + hide) — so the window HIDES to the tray and the process
  // stays alive (this very script keeps running). show() reshows it.
  function windowLifecycle(){
    var w = curWin();
    if (!w) { R.lifecycle = 'NO_WINDOW_API'; return Promise.resolve(); }
    R.lifecycle = {};
    return w.isVisible().then(function(v){ R.lifecycle.visibleBeforeClose = v; })
      .then(function(){ return w.close(); })
      .then(function(){ return delay(600); })
      .then(function(){ return w.isVisible(); }).then(function(v){ R.lifecycle.visibleAfterClose = v; })
      .then(function(){ return w.show(); })
      .then(function(){ return delay(300); })
      .then(function(){ return w.isVisible(); }).then(function(v){ R.lifecycle.visibleAfterReshow = v; })
      .then(function(){ R.lifecycle.survivedClose = true; })
      .catch(function(e){ R.lifecycle.err = String(e && e.message || e); });
  }
  // Seed an import snapshot directly through the `import_snapshot` command
  // (the old DropZone file-drop path that used to reach it is gone — the app
  // scans from the game). This exercises the same record path a file-drop
  // did: source='import' history row + the sell view rendering off it.
  function dropFixture(){
    return invk('import_snapshot', { inventoryJson: FIXTURE }).then(function(v){
      if (typeof v === 'string' && v.indexOf('ERR:') === 0) return v;
      return 'IMPORTED:' + v;
    });
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
    // C5 update check, endpoint overridden via TENNOWORTH_UPDATE_URL (offline
    // run: https to a refused port; malformed run: live JSON of the wrong
    // shape). Must resolve to {checked:true, available:false} — never reject.
    .then(function(){ return invk('check_update').then(function(v){ R.updateCheck = v; }); })
    .then(function(){ return invk('update_status').then(function(v){ R.updateStatus = v; }); })
    // The SPA's own mount handshake should have surfaced the banner iff an
    // update is available (debug-build happy-path run); and an explicit
    // install against a fixture manifest must REJECT (unreachable bundle URL /
    // garbage signature) without hurting the app — invk records the rejection
    // as an 'ERR:' string, and every later step still runs.
    .then(function(){
      R.updateBannerVisible = !!document.querySelector('[data-testid="update-banner"]');
      if (R.updateCheck && R.updateCheck.available) {
        return invk('install_update').then(function(v){ R.updateInstall = v; });
      }
      R.updateInstall = 'NOT_ATTEMPTED';
    })
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
    // (c) Seed an import snapshot → source='import' snapshot with item_count == 4.
    .then(function(){ return dropFixture().then(function(v){ R.dropResult = v; return delay(1500); }); })
    .then(function(){ return invk('list_snapshots', { limit: 50 }).then(function(v){ R.snapshotsAfterDrop = v; }); })
    // C6 (top_sellables): rank the imported snapshot × bundled market. With a
    // clean data dir (reserve 0) this is the deterministic 3-item ranking.
    .then(function(){ return invk('top_sellables', { limit: 5 }).then(function(v){ R.topSellables = v; }); })
    // C6 (notification): run the post-scan surface path against the latest
    // snapshot (probe-only, so it works with no game) → payload {count,total}.
    .then(function(){ return invk('debug_post_scan').then(function(v){ R.debugNotify = v; }); })
    // C6 (tray model): the labels the rebuild actually pushed + stored payload.
    .then(function(){ return invk('tray_state').then(function(v){ R.trayState = v; }); })
    // (a) Set reserve via the REAL input (now rendered) → set_setting.
    .then(function(){ return setReserve().then(function(via){ R.reserveSetVia = via; }); })
    .then(function(){ return invk('get_setting', { key: 'reserve-copies' }).then(function(v){ R.reserveAfterSet = v; }); })
    // C7 WFM listing session: the full lock-state machine, hermetic (no WFM
    // network — TENNOWORTH_JWT_PATH/TENNOWORTH_PENDING_PATH point at scratch,
    // and every plan item fails validation before any HTTP).
    .then(function(){ R.wfm = {}; return invkE('wfm_auth_status').then(function(v){ R.wfm.status0 = v; }); })
    // No login file → typed needs_login (the desktop analogue of serve's 401).
    .then(function(){ return invkE('submit_plan', { items: [] }).then(function(v){ R.wfm.planNoLogin = v; }); })
    // Real Sell CTA with no login → the login dialog opens (proactive check).
    .then(function(){
      var btn = document.querySelector('[data-testid="desktop-list"]');
      R.wfm.listBtnFound = !!btn;
      if (btn) btn.click();
      return delay(700);
    })
    .then(function(){
      var d = document.querySelector('[data-testid="wfm-login-dialog"]');
      R.wfm.loginDialogOpen = !!(d && d.open);
      if (d && d.open) d.close();
    })
    // Write a REAL encrypted envelope (production encrypt+persist), then the
    // same call sites must flip to needs_unlock.
    .then(function(){ return invkE('debug_write_login', { passphrase: 'probe-pass-123456' }).then(function(v){ R.wfm.wroteLogin = v; }); })
    .then(function(){ return invkE('wfm_auth_status').then(function(v){ R.wfm.status1 = v; }); })
    .then(function(){ return invkE('submit_plan', { items: [] }).then(function(v){ R.wfm.planLocked = v; }); })
    // Real CTA again → unlock dialog; drive the REAL form with a wrong
    // passphrase → bad_passphrase surfaces in the dialog, which stays open.
    .then(function(){
      var btn = document.querySelector('[data-testid="desktop-list"]');
      if (btn) btn.click();
      return delay(700);
    })
    .then(function(){
      var d = document.querySelector('[data-testid="wfm-unlock-dialog"]');
      R.wfm.unlockDialogOpen = !!(d && d.open);
      if (!(d && d.open)) return;
      var inp = d.querySelector('[data-testid="wfm-unlock-pass"]');
      var setter = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, 'value').set;
      setter.call(inp, 'wrong-passphrase');
      inp.dispatchEvent(new Event('input', { bubbles: true }));
      d.querySelector('form').requestSubmit();
      return delay(2500).then(function(){
        var err = d.querySelector('[data-testid="wfm-auth-error"]');
        R.wfm.unlockWrongPassError = err ? (err.innerText || '').slice(0, 120) : null;
        R.wfm.unlockDialogStillOpen = d.open;
        d.close();
      });
    })
    // Seed an unlocked session (probe-only, synthetic bundle — no network),
    // then the CTA opens the review modal with the staged fixture rows.
    .then(function(){ return invkE('debug_seed_unlocked').then(function(v){ R.wfm.seeded = v; }); })
    .then(function(){ return invkE('wfm_auth_status').then(function(v){ R.wfm.status2 = v; }); })
    .then(function(){
      var btn = document.querySelector('[data-testid="desktop-list"]');
      if (btn) btn.click();
      return delay(700);
    })
    .then(function(){
      var modal = document.querySelector('.modal');
      R.wfm.reviewModalOpen = !!modal;
      R.wfm.reviewModalRows = document.querySelectorAll('.modal tbody tr').length;
      var x = document.querySelector('.modal header .x');
      if (x) x.click();
      return delay(300);
    })
    // Offline plan execution: both items fail wfm-core validation BEFORE any
    // HTTP (price under the 5p floor; slug not in the catalog) — exercises the
    // full plan pipeline incl. pending-file seed + clean clear, no network.
    .then(function(){
      return invkE('submit_plan', { items: [
        { slug: 'ash_prime_set', platinum: 1, quantity: 1, order_type: 'sell', visible: false },
        { slug: 'not_a_real_slug', platinum: 10, quantity: 1, order_type: 'sell', visible: false }
      ]}).then(function(v){ R.wfm.planOffline = v; });
    })
    .then(function(){ return invkE('get_pending_plan').then(function(v){ R.wfm.pendingAfterPlan = v; }); })
    .then(function(){ return invkE('resume_pending_plan').then(function(v){ R.wfm.resumeNoPending = v; }); })
    .then(function(){ return invkE('wfm_logout').then(function(v){ R.wfm.logout = v; }); })
    .then(function(){ return invkE('wfm_auth_status').then(function(v){ R.wfm.status3 = v; }); })
    // Locked again with the envelope still on disk → needs_unlock, not login.
    .then(function(){ return invkE('submit_plan', { items: [] }).then(function(v){ R.wfm.planAfterLogout = v; }); })
    // Checkpoint the evidence BEFORE the lifecycle test — if close-to-tray were
    // broken and destroyed the window, the final report below would never write.
    .then(function(){ return invk('probe_report', { payload: JSON.stringify(R) }); })
    // C6 (lifecycle): close hides to tray (process survives), show reshows.
    .then(function(){ return windowLifecycle(); })
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

/// Substitute the run tag and JSON-escaped fixture into [`PROBE_JS`], ready to
/// hand to `WebviewWindowBuilder::initialization_script`. serde_json turns
/// the fixture `&str` into a quoted, escaped JS string literal so
/// `var FIXTURE = __FIXTURE__;` parses.
pub fn build_probe_script(runtag: &str) -> String {
    let fixture_literal = serde_json::to_string(PROBE_FIXTURE)
        .expect("probe fixture serializes to a JS string literal");
    PROBE_JS
        .replace("__RUNTAG__", runtag)
        .replace("__FIXTURE__", &fixture_literal)
}

/// Probe-only: run the full post-scan surface path (rebuild tray + fire the
/// notification) against whatever the latest snapshot is, so the notification
/// payload can be asserted without a running game (a seeded import_snapshot is
/// enough). Gated behind TENNOWORTH_PROBE so a normal build can't reach it.
#[tauri::command]
pub fn debug_post_scan(app: AppHandle) -> Result<Option<ScanNotification>, String> {
    if std::env::var("TENNOWORTH_PROBE").ok().as_deref() != Some("1") {
        return Err("debug_post_scan is probe-only".into());
    }
    post_scan_surfaces(&app);
    Ok(*guard(&app.state::<TrayState>().last_notification))
}

/// Probe-only: write a real encrypted login envelope (production encrypt +
/// persist path) so the hermetic probe can drive the needs_unlock /
/// bad_passphrase branches without a live WFM login.
#[tauri::command]
pub fn debug_write_login(
    session: State<'_, Arc<WfmSession>>,
    passphrase: String,
) -> Result<(), CmdError> {
    if std::env::var("TENNOWORTH_PROBE").ok().as_deref() != Some("1") {
        return Err(CmdError::internal("debug_write_login is probe-only"));
    }
    // Without the path override this would write a synthetic envelope over the
    // REAL ~/.config/wfminv/wfm-jwt.enc — a probe run must never be able to
    // clobber actual credentials, so refuse rather than fall back.
    if std::env::var_os("TENNOWORTH_JWT_PATH").is_none() {
        return Err(CmdError::internal(
            "debug_write_login requires TENNOWORTH_JWT_PATH (refusing to touch the real login file)",
        ));
    }
    session.debug_write_login(&passphrase)
}

/// Probe-only: flip the session to unlocked with a synthetic credential bundle
/// (empty catalog, fake JWT) — no network. Listing commands then exercise
/// their offline validation paths; anything that would hit WFM fails per-item.
#[tauri::command]
pub fn debug_seed_unlocked(session: State<'_, Arc<WfmSession>>) -> Result<(), CmdError> {
    if std::env::var("TENNOWORTH_PROBE").ok().as_deref() != Some("1") {
        return Err(CmdError::internal("debug_seed_unlocked is probe-only"));
    }
    session.debug_set_unlocked(Unlocked {
        jwt: "probe.jwt.value".into(),
        username: "probe".into(),
        platform: "pc".into(),
        catalog: Arc::new(BTreeMap::new()),
        id_to_name: Arc::new(BTreeMap::new()),
    });
    Ok(())
}

/// Probe-only: persist the evidence JSON to $TENNOWORTH_PROBE_OUT (and echo it
/// to stdout between markers so it is captured even without file access).
#[tauri::command]
pub fn probe_report(payload: String) -> Result<String, String> {
    // A literal "/tmp" default put the file at C:\tmp on Windows — a Unix path
    // silently resolving to the drive root. temp_dir() is %TEMP% there and /tmp
    // here.
    let out = std::env::var("TENNOWORTH_PROBE_OUT").unwrap_or_else(|_| {
        std::env::temp_dir()
            .join("tennoworth-probe.json")
            .to_string_lossy()
            .into_owned()
    });
    std::fs::write(&out, payload.as_bytes()).map_err(|e| e.to_string())?;
    // This command is called TWICE by design: once as a checkpoint before the
    // lifecycle test (so evidence survives if close-to-tray kills the window),
    // then again at the end. Label the markers so a consumer reading the first
    // match doesn't mistake the checkpoint for the whole run — the suffix is
    // additive, so greps for the bare marker still match both.
    let kind = if payload.contains("\"done\":true") {
        "FINAL"
    } else {
        "CHECKPOINT"
    };
    let mut so = std::io::stdout();
    let _ = writeln!(so, "PROBE_REPORT_FILE {out}");
    let _ = writeln!(so, "PROBE_REPORT_BEGIN {kind}");
    let _ = writeln!(so, "{payload}");
    let _ = writeln!(so, "PROBE_REPORT_END {kind}");
    let _ = so.flush();
    Ok(out)
}

/// Probe-only: close the app so the restart-persistence check can run two clean
/// launches without a human closing the window.
#[tauri::command]
pub fn probe_exit() {
    let mut so = std::io::stdout();
    let _ = writeln!(so, "PROBE_EXIT");
    let _ = so.flush();
    std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_millis(300));
        std::process::exit(0);
    });
}
