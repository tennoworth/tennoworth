//! Opt-in relic reward OCR overlay.
//!
//! The automatic path is deliberately event-driven: EE.log's reward marker
//! starts one bounded capture/OCR pass, and the global shortcut starts the
//! same pass when the game's buffered log arrives too late. Captured pixels
//! remain in memory and only normalized recognition results cross into the
//! overlay webview.

mod capture;
mod lifecycle;
mod presentation;
mod recognition;
#[cfg(test)]
mod tests;
use capture::{capture_warframe, CapturedFrame};
use lifecycle::CaptureLifecycle;
pub(crate) use presentation::prewarm_overlay_window;
use presentation::{
    hide_overlay, preferred_presentation_backend, present_overlay, push_overlay_result,
};
use recognition::{
    assemble_result, mark_bests, read_centered_reward_layout, read_dynamic_layout,
    read_expected_layout, LayoutRead, OcrWorker, RecognitionConsensus,
};

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{LazyLock, Mutex, RwLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_global_shortcut::GlobalShortcutExt;
use tauri_plugin_opener::OpenerExt;

use crate::persistence::Db;
use crate::services::market::MarketCache;
use crate::services::sellables::MarketData;

pub const EVENT_UPDATE: &str = "relic-overlay:update";
pub const EVENT_HIDE: &str = "relic-overlay:hide";
pub const EVENT_STATUS: &str = "relic-overlay:status";
const SETTINGS_KEY: &str = "relic-overlay-v1";
const DEFAULT_SHORTCUT: &str = "Ctrl+Shift+O";
const DEFAULT_REWARD_MARKER: &str = "Got rewards";
const REWARD_CLOSE_MARKER: &str = "Relic reward screen shut down";
const REWARD_SLOT_MARKER: &str = "ProjectionRewardChoice.lua: Missing icon data!";
const RECOMMENDATION_CONFIDENCE: f64 = 0.9;
// Warframe sizes this part of the reward UI from the viewport height. Keeping
// these dimensions height-relative makes the same layout work on 16:10 and
// ultrawide displays instead of stretching the card grid with the viewport.
const REWARD_SLOT_SPACING_PER_HEIGHT: f64 = 0.221;
const REWARD_CARD_WIDTH_PER_HEIGHT: f64 = 0.226;
const REWARD_TITLE_TARGET_WIDTH: u32 = 256;
const WARFRAME_DESIGN_ASPECT: f64 = 16.0 / 9.0;
static REWARD_MARKERS: LazyLock<RwLock<Vec<String>>> =
    LazyLock::new(|| RwLock::new(vec![DEFAULT_REWARD_MARKER.into()]));

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct OverlaySettings {
    pub enabled: bool,
    pub auto_detect: bool,
    pub shortcut: String,
    pub scale: f64,
    pub live_prices: bool,
    pub show_owned: bool,
    #[serde(default)]
    pub diagnostics: bool,
}

impl Default for OverlaySettings {
    fn default() -> Self {
        Self {
            enabled: false,
            auto_detect: true,
            shortcut: DEFAULT_SHORTCUT.into(),
            scale: 1.0,
            live_prices: true,
            show_owned: true,
            diagnostics: false,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OverlayStageTimings {
    pub capture_ms: u64,
    pub sparse_ocr_ms: u64,
    pub slot_ocr_ms: u64,
    pub matching_ms: u64,
    pub cached_display_ms: u64,
    pub live_price_refresh_ms: u64,
    pub total_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OverlayLastRun {
    pub outcome: String,
    pub trigger_source: String,
    pub expected_slots: usize,
    pub recognized_slots: usize,
    pub timings: OverlayStageTimings,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diagnostics_directory: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OverlayStatus {
    pub state: String,
    pub backend: String,
    pub presentation_backend: String,
    pub placement: String,
    pub ocr_ready: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_run: Option<OverlayLastRun>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl Default for OverlayStatus {
    fn default() -> Self {
        Self {
            state: "disabled".into(),
            backend: capture_backend_name().into(),
            presentation_backend: "tauri-window".into(),
            placement: "anchored".into(),
            ocr_ready: false,
            last_run: None,
            message: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct OverlayBox {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RelicOverlaySlot {
    pub index: usize,
    #[serde(rename = "box")]
    pub box_: OverlayBox,
    pub raw_text: String,
    pub name: Option<String>,
    pub slug: Option<String>,
    pub confidence: f64,
    pub cached_platinum: Option<u32>,
    pub live_platinum: Option<u32>,
    pub ducats: Option<u32>,
    pub owned: Option<u32>,
    pub best_platinum: bool,
    pub best_ducats: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RelicOverlayResult {
    pub capture_id: String,
    pub captured_at: String,
    pub scale: f64,
    pub slots: Vec<RelicOverlaySlot>,
}

pub struct OverlayState {
    settings: Mutex<OverlaySettings>,
    status: Mutex<OverlayStatus>,
    lifecycle: CaptureLifecycle,
    expected_slots: AtomicUsize,
    last_reward_marker: Mutex<Option<String>>,
    current_result: Mutex<Option<RelicOverlayResult>>,
    ocr: OcrWorker,
    diagnostics_root: PathBuf,
}

impl OverlayState {
    pub fn new(app: &AppHandle, db: &Db) -> Self {
        let settings = load_settings(db);
        let diagnostics_root = app
            .path()
            .app_cache_dir()
            .unwrap_or_else(|_| std::env::temp_dir().join("tennoworth"))
            .join("relic-overlay-diagnostics");
        let tessdata = app
            .path()
            .resource_dir()
            .map_err(|e| format!("ocr_unavailable: resolving resource directory: {e}"))
            .map(|p| p.join("tessdata"))
            .and_then(|p| {
                p.join("eng.traineddata")
                    .is_file()
                    .then_some(p)
                    .ok_or_else(|| {
                        "ocr_unavailable: bundled tessdata/eng.traineddata is missing".into()
                    })
            });
        let (ocr, ocr_result) = OcrWorker::start(tessdata);
        let status = OverlayStatus {
            state: if settings.enabled && ocr_result.is_ok() {
                "watching".into()
            } else if settings.enabled {
                "error".into()
            } else {
                "disabled".into()
            },
            ocr_ready: ocr_result.is_ok(),
            presentation_backend: preferred_presentation_backend().into(),
            message: ocr_result.err(),
            ..OverlayStatus::default()
        };
        Self {
            settings: Mutex::new(settings),
            status: Mutex::new(status),
            lifecycle: CaptureLifecycle::default(),
            expected_slots: AtomicUsize::new(0),
            last_reward_marker: Mutex::new(None),
            current_result: Mutex::new(None),
            ocr,
            diagnostics_root,
        }
    }

    fn set_status(&self, app: &AppHandle, state: &str, message: Option<String>) {
        let mut status = self.status.lock().unwrap_or_else(|e| e.into_inner());
        status.state = state.into();
        status.message = message;
        let _ = app.emit(EVENT_STATUS, status.clone());
    }

    fn finish_run(&self, app: &AppHandle, run: OverlayLastRun) {
        let mut status = self.status.lock().unwrap_or_else(|e| e.into_inner());
        status.last_run = Some(run);
        let _ = app.emit(EVENT_STATUS, status.clone());
    }

    fn set_presentation_backend(&self, app: &AppHandle, backend: &str) {
        let mut status = self.status.lock().unwrap_or_else(|e| e.into_inner());
        status.presentation_backend = backend.into();
        let _ = app.emit(EVENT_STATUS, status.clone());
    }
}

fn load_settings(db: &Db) -> OverlaySettings {
    db.get_setting(SETTINGS_KEY)
        .ok()
        .flatten()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn validate_settings(mut settings: OverlaySettings) -> Result<OverlaySettings, String> {
    if !settings.scale.is_finite() {
        return Err("overlay scale must be a number".into());
    }
    settings.scale = settings.scale.clamp(0.75, 1.5);
    settings.shortcut = settings.shortcut.trim().to_string();
    if settings.shortcut.is_empty() || settings.shortcut.len() > 80 {
        return Err("choose a valid overlay shortcut".into());
    }
    Ok(settings)
}

#[tauri::command]
pub fn get_overlay_settings(db: State<'_, Db>) -> OverlaySettings {
    load_settings(&db)
}

#[tauri::command]
pub fn update_overlay_settings(
    app: AppHandle,
    db: State<'_, Db>,
    state: State<'_, OverlayState>,
    settings: OverlaySettings,
) -> Result<OverlaySettings, String> {
    let settings = validate_settings(settings)?;
    let previous = state
        .settings
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    app.global_shortcut()
        .unregister_all()
        .map_err(|e| format!("unregistering old shortcut: {e}"))?;
    if settings.enabled {
        if let Err(error) = app.global_shortcut().register(settings.shortcut.as_str()) {
            if previous.enabled {
                let _ = app.global_shortcut().register(previous.shortcut.as_str());
            }
            return Err(format!(
                "shortcut {} is unavailable: {error}",
                settings.shortcut
            ));
        }
    }
    let raw = serde_json::to_string(&settings).map_err(|e| e.to_string())?;
    if let Err(error) = db.set_setting(SETTINGS_KEY, &raw) {
        let _ = app.global_shortcut().unregister_all();
        if previous.enabled {
            let _ = app.global_shortcut().register(previous.shortcut.as_str());
        }
        return Err(error.to_string());
    }
    *state.settings.lock().unwrap_or_else(|e| e.into_inner()) = settings.clone();
    let ocr_ready = state
        .status
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .ocr_ready;
    state.set_status(
        &app,
        if settings.enabled && ocr_ready {
            "watching"
        } else if settings.enabled {
            "error"
        } else {
            "disabled"
        },
        (!ocr_ready && settings.enabled)
            .then(|| "ocr_unavailable: bundled English OCR model did not initialize".into()),
    );
    if !settings.enabled {
        hide_overlay(&app);
    }
    Ok(settings)
}

#[tauri::command]
pub fn overlay_status(state: State<'_, OverlayState>) -> OverlayStatus {
    state
        .status
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

#[tauri::command]
pub fn current_overlay_result(state: State<'_, OverlayState>) -> Option<RelicOverlayResult> {
    state
        .current_result
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

#[tauri::command]
pub fn preview_relic_overlay(app: AppHandle, state: State<'_, OverlayState>) -> Result<(), String> {
    let monitor = app
        .primary_monitor()
        .map_err(|e| format!("reading primary monitor: {e}"))?
        .ok_or_else(|| "no primary monitor is available".to_string())?;
    let monitor_size = monitor.size();
    let monitor_position = monitor.position();
    let width = (monitor_size.width as f64 * 0.52).round().max(720.0) as u32;
    let height = 150u32;
    let x = monitor_position.x + (monitor_size.width.saturating_sub(width) / 2) as i32;
    let y = monitor_position.y + (monitor_size.height as f64 * 0.48).round() as i32;
    let names = [
        ("Forma Blueprint", None, None, true, false),
        (
            "Lavos Prime Chassis Blueprint",
            Some(12),
            Some(15),
            false,
            true,
        ),
        ("Dual Zoren Prime Handle", Some(24), Some(45), true, true),
        (
            "Revenant Prime Systems Blueprint",
            Some(8),
            Some(15),
            false,
            false,
        ),
    ];
    let slots = names
        .into_iter()
        .enumerate()
        .map(
            |(index, (name, platinum, ducats, best_platinum, best_ducats))| RelicOverlaySlot {
                index,
                box_: OverlayBox {
                    x: index as f64 * 0.25,
                    y: 0.05,
                    width: 0.25,
                    height: 0.9,
                },
                raw_text: name.into(),
                name: Some(name.into()),
                slug: None,
                confidence: 1.0,
                cached_platinum: platinum,
                live_platinum: None,
                ducats,
                owned: None,
                best_platinum,
                best_ducats,
            },
        )
        .collect();
    let capture_id = format!("preview-{}", unix_millis());
    let result = RelicOverlayResult {
        capture_id: capture_id.clone(),
        captured_at: unix_millis().to_string(),
        scale: state
            .settings
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .scale,
        slots,
    };
    *state
        .current_result
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = Some(result.clone());
    state.lifecycle.present(capture_id.clone());
    present_overlay(&app, x, y, width, height, &result)?;
    state.set_status(&app, "showing", Some("overlay preview".into()));
    let app_for_hide = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(8));
        let state = app_for_hide.state::<OverlayState>();
        if state.lifecycle.is_current(&capture_id) {
            hide_overlay(&app_for_hide);
            state.set_status(&app_for_hide, "watching", None);
        }
    });
    Ok(())
}

#[tauri::command]
pub fn setup_overlay_capture(
    app: AppHandle,
    state: State<'_, OverlayState>,
) -> Result<OverlayStatus, String> {
    let enabled = state
        .settings
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .enabled;
    if !enabled {
        return Err("enable the relic overlay first".into());
    }
    if !state
        .status
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .ocr_ready
    {
        return Err("ocr_unavailable: bundled English OCR model did not initialize".into());
    }
    state.set_status(
        &app,
        "recognizing",
        Some("checking screen-capture access".into()),
    );
    if let Err(error) = capture_warframe() {
        state.set_status(&app, "error", Some(error.clone()));
        return Err(error);
    }
    state.set_status(&app, "watching", None);
    Ok(state
        .status
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone())
}

#[tauri::command]
pub fn open_overlay_diagnostics(
    app: AppHandle,
    state: State<'_, OverlayState>,
) -> Result<(), String> {
    std::fs::create_dir_all(&state.diagnostics_root)
        .map_err(|e| format!("creating diagnostics directory: {e}"))?;
    app.opener()
        .open_path(state.diagnostics_root.display().to_string(), None::<&str>)
        .map_err(|e| format!("opening diagnostics directory: {e}"))
}

#[tauri::command]
pub fn clear_overlay_diagnostics(state: State<'_, OverlayState>) -> Result<(), String> {
    let Ok(entries) = std::fs::read_dir(&state.diagnostics_root) else {
        return Ok(());
    };
    for entry in entries {
        let path = entry
            .map_err(|e| format!("reading diagnostics directory: {e}"))?
            .path();
        if path.is_dir() {
            std::fs::remove_dir_all(&path)
                .map_err(|e| format!("clearing diagnostics run {}: {e}", path.display()))?;
        } else {
            std::fs::remove_file(&path)
                .map_err(|e| format!("clearing diagnostics file {}: {e}", path.display()))?;
        }
    }
    Ok(())
}

#[tauri::command]
pub fn ocr_boot_probe(state: State<'_, OverlayState>) -> Result<(), String> {
    let status = state.status.lock().unwrap_or_else(|e| e.into_inner());
    if status.ocr_ready {
        Ok(())
    } else {
        Err(status
            .message
            .clone()
            .unwrap_or_else(|| "ocr_unavailable: initialization failed".into()))
    }
}

#[tauri::command]
pub fn scan_overlay_now(app: AppHandle) -> Result<(), String> {
    // The settings button is a hand-off to the game: leaving the dashboard in
    // front makes a correctly positioned transparent overlay look embedded in
    // Tennoworth instead. A global-shortcut capture already has Warframe in
    // front and does not take this path.
    if let Some(window) = app.get_webview_window("main") {
        window
            .hide()
            .map_err(|e| format!("hiding Tennoworth before capture: {e}"))?;
    }
    trigger_capture(&app, "settings button")
}

pub fn register_configured_shortcut(app: &AppHandle) {
    let Some(state) = app.try_state::<OverlayState>() else {
        return;
    };
    let settings = state
        .settings
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    if settings.enabled {
        if let Err(error) = app.global_shortcut().register(settings.shortcut.as_str()) {
            state.set_status(
                app,
                "error",
                Some(format!(
                    "shortcut {} is unavailable: {error}",
                    settings.shortcut
                )),
            );
        }
    }
}

pub fn handle_log_line(app: &AppHandle, line: &str) {
    let Some(state) = app.try_state::<OverlayState>() else {
        return;
    };
    let settings = state
        .settings
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    if settings.enabled && line.contains(REWARD_CLOSE_MARKER) {
        state.expected_slots.store(0, Ordering::Release);
        hide_overlay(app);
        state.set_status(app, "watching", None);
        return;
    }
    let reward_line = REWARD_MARKERS
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .any(|marker| line.contains(marker));
    if reward_line {
        *state
            .last_reward_marker
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Some(line.to_string());
        state.expected_slots.store(0, Ordering::Release);
    } else if line.contains(REWARD_SLOT_MARKER) {
        let _ = state
            .expected_slots
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                Some((count + 1).min(4))
            });
    }
    if settings.enabled && settings.auto_detect && reward_line {
        let _ = trigger_capture(app, "eelog");
    }
}

#[allow(
    clippy::indexing_slicing,
    reason = "start comes from rposition on the line list, so lines[start] and lines[start+1..] are in bounds"
)]
fn latest_active_reward_batch(text: &str) -> Option<(String, usize)> {
    let markers = REWARD_MARKERS.read().unwrap_or_else(|e| e.into_inner());
    let lines: Vec<&str> = text.lines().collect();
    let start = lines
        .iter()
        .rposition(|line| markers.iter().any(|marker| line.contains(marker.as_str())))?;
    if lines[start + 1..]
        .iter()
        .any(|line| line.contains(REWARD_CLOSE_MARKER))
    {
        return None;
    }
    let slots = lines[start + 1..]
        .iter()
        .filter(|line| line.contains(REWARD_SLOT_MARKER))
        .count()
        .min(4);
    (slots > 0).then(|| (lines[start].to_string(), slots))
}

pub fn handle_log_snapshot(app: &AppHandle, text: &str) {
    let Some((marker, slots)) = latest_active_reward_batch(text) else {
        return;
    };
    let state = app.state::<OverlayState>();
    let mut previous = state
        .last_reward_marker
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if previous.as_deref() == Some(&marker) {
        return;
    }
    *previous = Some(marker);
    drop(previous);
    state.expected_slots.store(slots, Ordering::Release);
    let settings = state
        .settings
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    if settings.enabled && settings.auto_detect {
        let _ = trigger_capture(app, "eelog-snapshot");
    }
}

pub fn install_reward_markers(markers: &[String]) -> Result<(), String> {
    if markers.is_empty() {
        return Ok(());
    }
    if markers.len() > 8
        || markers
            .iter()
            .any(|marker| marker.trim().is_empty() || marker.len() > 120)
    {
        return Err(
            "reward_log_markers must contain 1–8 non-empty strings of at most 120 bytes".into(),
        );
    }
    *REWARD_MARKERS.write().unwrap_or_else(|e| e.into_inner()) = markers
        .iter()
        .map(|marker| marker.trim().to_string())
        .collect();
    Ok(())
}

pub fn trigger_capture(app: &AppHandle, source: &str) -> Result<(), String> {
    let state = app.state::<OverlayState>();
    if !state
        .settings
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .enabled
    {
        return Err("relic overlay is disabled".into());
    }
    if !state
        .status
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .ocr_ready
    {
        return Err("ocr_unavailable: bundled English OCR model did not initialize".into());
    }
    if !state.lifecycle.try_begin() {
        return Err("a relic recognition pass is already running".into());
    }
    state.set_status(app, "recognizing", Some(format!("triggered by {source}")));
    let triggered_at = Instant::now();
    let source = source.to_string();
    let app = app.clone();
    std::thread::Builder::new()
        .name("relic-capture".into())
        .spawn(move || {
            if let Err(error) = capture_and_recognize(&app, &source, triggered_at) {
                eprintln!("tennoworth: relic scan ({source}) failed: {error}");
                app.state::<OverlayState>()
                    .set_status(&app, "error", Some(error));
            }
            app.state::<OverlayState>().lifecycle.finish();
        })
        .map_err(|e| {
            state.lifecycle.finish();
            format!("starting capture worker: {e}")
        })?;
    Ok(())
}

fn create_diagnostics_run(root: &Path, scan_id: &str) -> Result<PathBuf, String> {
    std::fs::create_dir_all(root).map_err(|e| format!("creating diagnostics root: {e}"))?;
    let mut runs: Vec<_> = std::fs::read_dir(root)
        .map_err(|e| format!("reading diagnostics root: {e}"))?
        .filter_map(Result::ok)
        .filter(|entry| entry.path().is_dir())
        .collect();
    runs.sort_by_key(|entry| entry.file_name());
    for old in runs.iter().take(runs.len().saturating_sub(9)) {
        let _ = std::fs::remove_dir_all(old.path());
    }
    let dir = root.join(scan_id);
    std::fs::create_dir_all(&dir).map_err(|e| format!("creating diagnostics run: {e}"))?;
    Ok(dir)
}

fn diagnostics_attempt_dir(run_dir: Option<&Path>, attempt: usize) -> Option<PathBuf> {
    let dir = run_dir?.join(format!("attempt-{}", attempt + 1));
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

fn write_scan_frame_debug(run_dir: Option<&Path>, attempt: usize, frame: &CapturedFrame) {
    if let Some(dir) = diagnostics_attempt_dir(run_dir, attempt) {
        let image = frame.image.clone();
        let _ = std::thread::Builder::new()
            .name("relic-diagnostics".into())
            .spawn(move || {
                let _ = image.save(dir.join("warframe.png"));
            });
    }
}

fn write_run_diagnostics(
    run_dir: Option<&Path>,
    timings: &OverlayStageTimings,
    result: Option<&RelicOverlayResult>,
) {
    let Some(dir) = run_dir else {
        return;
    };
    if let Ok(json) = serde_json::to_vec_pretty(timings) {
        let _ = std::fs::write(dir.join("timings.json"), json);
    }
    if let Some(result) = result {
        if let Ok(json) = serde_json::to_vec_pretty(result) {
            let _ = std::fs::write(dir.join("resolved-results.json"), json);
        }
    }
}

fn capture_and_recognize(
    app: &AppHandle,
    source: &str,
    run_started: Instant,
) -> Result<(), String> {
    let state = app.state::<OverlayState>();
    let cache = app.state::<MarketCache>();
    let db = app.state::<Db>();
    let market = MarketData::load(&cache);
    let catalog = market.overlay_catalog();
    let scan_id = format!("{}-{source}-{}", std::process::id(), unix_millis());
    let settings = state
        .settings
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    let expected_slots = state.expected_slots.load(Ordering::Acquire);
    let run_dir = if settings.diagnostics {
        create_diagnostics_run(&state.diagnostics_root, &scan_id).ok()
    } else {
        None
    };
    let diagnostic_path = run_dir.as_ref().map(|path| path.display().to_string());
    let mut timings = OverlayStageTimings::default();
    if let Some(dir) = &run_dir {
        let marker = state
            .last_reward_marker
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .and_then(|line| {
                REWARD_MARKERS
                    .read()
                    .unwrap_or_else(|e| e.into_inner())
                    .iter()
                    .find(|marker| line.contains(marker.as_str()))
                    .cloned()
            });
        let context = serde_json::json!({
            "triggerSource": source,
            "expectedSlots": expected_slots,
            "rewardMarker": marker,
            "platform": std::env::consts::OS,
            "architecture": std::env::consts::ARCH,
            "captureBackend": capture_backend_name(),
        });
        if let Ok(json) = serde_json::to_vec_pretty(&context) {
            let _ = std::fs::write(dir.join("context.json"), json);
        }
    }
    eprintln!(
        "tennoworth: relic scan {scan_id} started with {} expected slots",
        expected_slots
    );
    let mut consensus = RecognitionConsensus::default();
    let mut recognized: Option<(CapturedFrame, LayoutRead)> = None;
    let mut last_frame = None;
    let mut last_error = None;
    for attempt in 0..3 {
        let capture_started = Instant::now();
        match capture_warframe() {
            Ok(frame) => {
                timings.capture_ms += elapsed_ms(capture_started);
                write_scan_frame_debug(run_dir.as_deref(), attempt, &frame);
                let debug_dir = diagnostics_attempt_dir(run_dir.as_deref(), attempt);
                match read_centered_reward_layout(
                    &state.ocr,
                    &frame,
                    &catalog,
                    debug_dir.as_deref(),
                    &mut timings,
                ) {
                    Ok(read) => {
                        consensus.observe(read);
                    }
                    Err(error) => last_error = Some(error),
                }
                let current_expected = state.expected_slots.load(Ordering::Acquire);
                if (1..=4).contains(&current_expected)
                    && consensus.resolve(current_expected, true).is_none()
                {
                    match read_expected_layout(
                        &state.ocr,
                        &frame,
                        &catalog,
                        current_expected,
                        debug_dir.as_deref(),
                        &mut timings,
                    ) {
                        Ok(read) => {
                            consensus.observe(read);
                        }
                        Err(error) => last_error = Some(error),
                    }
                }
                // Full-frame sparse OCR is the compatibility fallback, not the
                // normal hot path. Run it once only after the inexpensive
                // centered crops have had time to catch a drawing transition.
                let current_expected = state.expected_slots.load(Ordering::Acquire);
                if consensus.resolve(current_expected, true).is_none() && attempt == 2 {
                    match read_dynamic_layout(
                        &state.ocr,
                        &frame,
                        &catalog,
                        &state.expected_slots,
                        run_dir.as_deref(),
                        attempt,
                        &mut timings,
                    ) {
                        Ok(read) => {
                            consensus.observe(read);
                        }
                        Err(error) => {
                            let preserve_catalog_error =
                                last_error.as_deref().is_some_and(|previous| {
                                    previous.starts_with("catalog_match_incomplete:")
                                });
                            if !preserve_catalog_error {
                                last_error = Some(error);
                            }
                        }
                    }
                }
                let current_expected = state.expected_slots.load(Ordering::Acquire);
                if let Some(read) = consensus.resolve(current_expected, true) {
                    recognized = Some((frame, read));
                    break;
                }
                last_frame = Some(frame);
            }
            Err(error) => {
                timings.capture_ms += elapsed_ms(capture_started);
                last_error = Some(error);
            }
        }
        if attempt < 2 {
            std::thread::sleep(Duration::from_millis(if attempt == 0 { 75 } else { 125 }));
        }
    }
    let Some((frame, read)) = recognized else {
        let current_expected = state.expected_slots.load(Ordering::Acquire);
        let partial = consensus.resolve(current_expected, false);
        let recognized_slots = partial.as_ref().map_or(0, |read| read.matches.len());
        let expected_slots = if (1..=4).contains(&current_expected) {
            current_expected
        } else {
            partial.as_ref().map_or(0, |read| read.layout.count)
        };
        let cause = if recognized_slots > 0 && expected_slots > recognized_slots {
            format!(
                "catalog_match_incomplete: recognized {recognized_slots} of {expected_slots} expected reward names"
            )
        } else {
            last_error.unwrap_or_else(|| "reward recognition failed".into())
        };
        let mut error = format!("{cause}; retry while the reward names are visible");
        let partial_for_display = partial.filter(|read| {
            (1..=4).contains(&expected_slots)
                && read.layout.count == expected_slots
                && recognized_slots > 0
                && recognized_slots < expected_slots
        });
        if let (Some(frame), Some(read)) = (last_frame, partial_for_display) {
            let owned = if settings.show_owned {
                market.overlay_owned(&db)
            } else {
                None
            };
            let capture_id = scan_id;
            let (result, overlay_geometry) = assemble_result(
                &frame,
                read,
                &settings,
                expected_slots,
                |slug| market.overlay_market_facts(slug),
                owned.as_ref(),
                capture_id.clone(),
            );
            state.lifecycle.present(capture_id.clone());
            *state
                .current_result
                .lock()
                .unwrap_or_else(|e| e.into_inner()) = Some(result.clone());
            match present_overlay(
                app,
                overlay_geometry.0,
                overlay_geometry.1,
                overlay_geometry.2,
                overlay_geometry.3,
                &result,
            ) {
                Ok(()) => {
                    timings.cached_display_ms = elapsed_ms(run_started);
                    timings.total_ms = elapsed_ms(run_started);
                    write_run_diagnostics(run_dir.as_deref(), &timings, Some(&result));
                    state.set_status(app, "showing", Some(error.clone()));
                    state.finish_run(
                        app,
                        OverlayLastRun {
                            outcome: error,
                            trigger_source: source.into(),
                            expected_slots,
                            recognized_slots,
                            timings,
                            diagnostics_directory: diagnostic_path,
                        },
                    );
                    eprintln!(
                        "tennoworth: relic scan {capture_id} showing {recognized_slots} of {expected_slots} recognized slots without recommendations"
                    );
                    schedule_overlay_hide(app, capture_id);
                    return Ok(());
                }
                Err(display_error) => {
                    error = format!("{error}; displaying partial results failed: {display_error}");
                }
            }
        }
        timings.total_ms = elapsed_ms(run_started);
        write_run_diagnostics(run_dir.as_deref(), &timings, None);
        state.finish_run(
            app,
            OverlayLastRun {
                outcome: error.clone(),
                trigger_source: source.into(),
                expected_slots,
                recognized_slots,
                timings,
                diagnostics_directory: diagnostic_path,
            },
        );
        return Err(error);
    };
    let owned = if settings.show_owned {
        market.overlay_owned(&db)
    } else {
        None
    };
    let capture_id = scan_id;
    let (mut result, overlay_geometry) = assemble_result(
        &frame,
        read,
        &settings,
        expected_slots,
        |slug| market.overlay_market_facts(slug),
        owned.as_ref(),
        capture_id.clone(),
    );
    state.lifecycle.present(capture_id.clone());
    *state
        .current_result
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = Some(result.clone());
    if let Err(error) = present_overlay(
        app,
        overlay_geometry.0,
        overlay_geometry.1,
        overlay_geometry.2,
        overlay_geometry.3,
        &result,
    ) {
        timings.total_ms = elapsed_ms(run_started);
        write_run_diagnostics(run_dir.as_deref(), &timings, Some(&result));
        state.finish_run(
            app,
            OverlayLastRun {
                outcome: error.clone(),
                trigger_source: source.into(),
                expected_slots,
                recognized_slots: result.slots.len(),
                timings,
                diagnostics_directory: diagnostic_path,
            },
        );
        return Err(error);
    }
    timings.cached_display_ms = elapsed_ms(run_started);
    state.set_status(app, "showing", None);
    eprintln!(
        "tennoworth: relic scan {capture_id} showing {} recognized slots",
        result.slots.len()
    );

    if settings.live_prices {
        let live_started = Instant::now();
        let queries: Vec<wfm_core::trading::live_top::LiveTopQuery> = result
            .slots
            .iter()
            .filter_map(|slot| {
                slot.slug
                    .as_ref()
                    .map(|slug| wfm_core::trading::live_top::LiveTopQuery {
                        slug: slug.clone(),
                        rank: None,
                        subtype: None,
                    })
            })
            .collect();
        if !queries.is_empty() {
            if let Ok(live) =
                wfm_core::trading::live_top::fetch_live_tops("pc", None, &queries, |_, _| {})
            {
                for slot in &mut result.slots {
                    slot.live_platinum = slot
                        .slug
                        .as_ref()
                        .and_then(|slug| live.iter().find(|row| &row.slug == slug))
                        .and_then(|row| row.low_sell)
                        .map(|price| price.round() as u32);
                }
                mark_bests(&mut result.slots);
                if state.lifecycle.is_current(&capture_id) {
                    *state
                        .current_result
                        .lock()
                        .unwrap_or_else(|e| e.into_inner()) = Some(result.clone());
                    let _ = push_overlay_result(app, &result);
                }
            }
        }
        timings.live_price_refresh_ms = elapsed_ms(live_started);
    }

    timings.total_ms = elapsed_ms(run_started);
    write_run_diagnostics(run_dir.as_deref(), &timings, Some(&result));
    state.finish_run(
        app,
        OverlayLastRun {
            outcome: "success".into(),
            trigger_source: source.into(),
            expected_slots,
            recognized_slots: result.slots.len(),
            timings,
            diagnostics_directory: diagnostic_path,
        },
    );

    schedule_overlay_hide(app, capture_id);
    Ok(())
}

fn schedule_overlay_hide(app: &AppHandle, capture_id: String) {
    let app_for_hide = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(20));
        let state = app_for_hide.state::<OverlayState>();
        if state.lifecycle.is_current(&capture_id) {
            hide_overlay(&app_for_hide);
            state.set_status(&app_for_hide, "watching", None);
        }
    });
}

fn unix_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

fn elapsed_ms(started: Instant) -> u64 {
    started.elapsed().as_millis().min(u64::MAX as u128) as u64
}

pub fn capture_backend_name() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        "windows-window"
    }
    #[cfg(target_os = "linux")]
    {
        // Capture talks X11 directly - under Wayland that means XWayland, not
        // the portal. Report the truth so the settings UI cannot claim a
        // portal/PipeWire backend that does not exist.
        if std::env::var_os("WAYLAND_DISPLAY").is_some() {
            "wayland-xwayland"
        } else {
            "x11-window"
        }
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        "unsupported"
    }
}
