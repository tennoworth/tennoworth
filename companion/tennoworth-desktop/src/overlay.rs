//! Opt-in relic reward OCR overlay.
//!
//! The automatic path is deliberately event-driven: EE.log's reward marker
//! starts one bounded capture/OCR pass, and the global shortcut starts the
//! same pass when the game's buffered log arrives too late. Captured pixels
//! remain in memory and only normalized recognition results cross into the
//! overlay webview.

use std::collections::{BTreeMap, HashMap};
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, LazyLock, Mutex, RwLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use image::{imageops::FilterType, DynamicImage, ImageFormat, RgbaImage};
use serde::{Deserialize, Serialize};
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, State, WebviewUrl,
    WebviewWindowBuilder,
};
use tauri_plugin_global_shortcut::GlobalShortcutExt;
use tauri_plugin_opener::OpenerExt;

use crate::db::Db;
use crate::market::MarketCache;
use crate::sellables::{MarketData, OverlayCatalogItem, OverlayMarketFacts};

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

struct OcrRequest {
    png: Vec<u8>,
    mode: OcrMode,
    reply: mpsc::SyncSender<Result<String, String>>,
}

#[derive(Clone, Copy)]
enum OcrMode {
    SparseTsv,
    SingleLine,
}

struct OcrWorker {
    tx: mpsc::Sender<OcrRequest>,
}

impl OcrWorker {
    fn start(tessdata: Result<PathBuf, String>) -> (Self, Result<(), String>) {
        let (tx, rx) = mpsc::channel::<OcrRequest>();
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let _ = std::thread::Builder::new()
            .name("relic-ocr".into())
            .spawn(move || {
                let mut engine = tessdata.and_then(|path| {
                    let data = path.to_str().ok_or_else(|| {
                        "ocr_unavailable: tessdata path is not valid UTF-8".to_string()
                    })?;
                    // Tauri canonicalizes bundled resources to a Windows verbatim
                    // path. Tesseract's C API cannot open the `\\?\` form.
                    #[cfg(target_os = "windows")]
                    let data = data.strip_prefix(r"\\?\").unwrap_or(data);
                    leptess::LepTess::new(Some(data), "eng").map_err(|e| {
                        format!(
                            "ocr_unavailable: starting Tesseract with bundled eng.traineddata: {e}"
                        )
                    })
                });
                let _ = ready_tx.send(engine.as_ref().map(|_| ()).map_err(Clone::clone));
                while let Ok(req) = rx.recv() {
                    let answer = match engine.as_mut() {
                        Ok(ocr) => {
                            let page_mode = match req.mode {
                                OcrMode::SparseTsv => "11",
                                OcrMode::SingleLine => "6",
                            };
                            let _ =
                                ocr.set_variable(leptess::Variable::TesseditPagesegMode, page_mode);
                            ocr.set_image_from_mem(&req.png)
                                .map_err(|e| format!("loading capture into OCR: {e}"))
                                .and_then(|_| match req.mode {
                                    OcrMode::SparseTsv => ocr
                                        .get_tsv_text(0)
                                        .map_err(|e| format!("locating reward text: {e}")),
                                    OcrMode::SingleLine => ocr
                                        .get_utf8_text()
                                        .map_err(|e| format!("recognizing reward text: {e}")),
                                })
                        }
                        Err(message) => Err(message.clone()),
                    };
                    let _ = req.reply.send(answer);
                }
            });
        let ready = ready_rx
            .recv_timeout(Duration::from_secs(10))
            .unwrap_or_else(|_| Err("ocr_unavailable: Tesseract initialization timed out".into()));
        (Self { tx }, ready)
    }

    fn recognize(&self, png: Vec<u8>, mode: OcrMode) -> Result<String, String> {
        let (tx, rx) = mpsc::sync_channel(1);
        self.tx
            .send(OcrRequest {
                png,
                mode,
                reply: tx,
            })
            .map_err(|_| "OCR worker stopped".to_string())?;
        rx.recv_timeout(Duration::from_secs(5))
            .map_err(|_| "OCR timed out".to_string())?
    }
}

pub struct OverlayState {
    settings: Mutex<OverlaySettings>,
    status: Mutex<OverlayStatus>,
    busy: AtomicBool,
    expected_slots: AtomicUsize,
    last_reward_marker: Mutex<Option<String>>,
    current_capture: Mutex<Option<String>>,
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
            busy: AtomicBool::new(false),
            expected_slots: AtomicUsize::new(0),
            last_reward_marker: Mutex::new(None),
            current_capture: Mutex::new(None),
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
pub fn preview_relic_overlay(
    app: AppHandle,
    state: State<'_, OverlayState>,
) -> Result<(), String> {
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
        ("Lavos Prime Chassis Blueprint", Some(12), Some(15), false, true),
        ("Dual Zoren Prime Handle", Some(24), Some(45), true, true),
        ("Revenant Prime Systems Blueprint", Some(8), Some(15), false, false),
    ];
    let slots = names
        .into_iter()
        .enumerate()
        .map(|(index, (name, platinum, ducats, best_platinum, best_ducats))| {
            RelicOverlaySlot {
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
            }
        })
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
    *state
        .current_capture
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = Some(capture_id.clone());
    present_overlay(&app, x, y, width, height, &result)?;
    state.set_status(&app, "showing", Some("overlay preview".into()));
    let app_for_hide = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(8));
        let state = app_for_hide.state::<OverlayState>();
        let current = state
            .current_capture
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        if current.as_deref() == Some(&capture_id) {
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
    if state.busy.swap(true, Ordering::AcqRel) {
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
            app.state::<OverlayState>()
                .busy
                .store(false, Ordering::Release);
        })
        .map_err(|e| {
            state.busy.store(false, Ordering::Release);
            format!("starting capture worker: {e}")
        })?;
    Ok(())
}

struct CapturedFrame {
    image: RgbaImage,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

#[derive(Debug, Clone, Copy, Serialize)]
struct NormalizedRect {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

#[derive(Debug, Clone, Serialize)]
struct RewardSlotRect {
    index: usize,
    card: NormalizedRect,
    title: NormalizedRect,
}

#[derive(Debug, Clone, Serialize)]
struct RewardLayout {
    count: usize,
    confidence: f64,
    slots: Vec<RewardSlotRect>,
}

struct LayoutRead {
    layout: RewardLayout,
    matches: Vec<(usize, FoundMatch)>,
}

#[derive(Debug, Clone)]
struct TsvLine {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    text: String,
}

fn pixel_rect(image: &RgbaImage, rect: NormalizedRect) -> (u32, u32, u32, u32) {
    let x = (rect.x * image.width() as f64)
        .round()
        .clamp(0.0, image.width().saturating_sub(1) as f64) as u32;
    let y = (rect.y * image.height() as f64)
        .round()
        .clamp(0.0, image.height().saturating_sub(1) as f64) as u32;
    let width = (rect.width * image.width() as f64)
        .round()
        .clamp(1.0, image.width().saturating_sub(x) as f64) as u32;
    let height = (rect.height * image.height() as f64)
        .round()
        .clamp(1.0, image.height().saturating_sub(y) as f64) as u32;
    (x, y, width, height)
}

fn encode_crop(image: &RgbaImage, rect: NormalizedRect) -> Result<Vec<u8>, String> {
    let (x, y, width, height) = pixel_rect(image, rect);
    let mut crop = image::imageops::crop_imm(image, x, y, width, height).to_image();
    // Give Tesseract roughly the same glyph size at 720p, 1080p, 1440p and 4K.
    // This also bounds the amount of pixel data sent through Leptonica at 4K.
    let target_height =
        ((height as f64 * REWARD_TITLE_TARGET_WIDTH as f64 / width as f64).round() as u32).max(1);
    if crop.width() != REWARD_TITLE_TARGET_WIDTH {
        crop = image::imageops::resize(
            &crop,
            REWARD_TITLE_TARGET_WIDTH,
            target_height,
            FilterType::Triangle,
        );
    }
    // Reward labels are white over translucent cards whose gold/white item art
    // frequently merges into the glyphs. Per-channel thresholding preserves
    // the bright label edges while discarding most of that background noise.
    for pixel in crop.pixels_mut() {
        for channel in &mut pixel.0[..3] {
            *channel = if *channel >= 178 { 255 } else { 0 };
        }
        pixel.0[3] = 255;
    }
    let mut cursor = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(crop)
        .write_to(&mut cursor, ImageFormat::Png)
        .map_err(|e| format!("encoding reward title crop: {e}"))?;
    Ok(cursor.into_inner())
}

fn encode_frame(image: &RgbaImage) -> Result<Vec<u8>, String> {
    let mut cursor = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(image.clone())
        .write_to(&mut cursor, ImageFormat::Png)
        .map_err(|e| format!("encoding Warframe capture: {e}"))?;
    Ok(cursor.into_inner())
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

#[allow(
    clippy::indexing_slicing,
    reason = "fields.len() == 12 is checked before every index, and the fields[0] guard filters the rest"
)]
fn parse_tsv_lines(tsv: &str) -> Vec<TsvLine> {
    #[derive(Default)]
    struct LineBuilder {
        left: u32,
        top: u32,
        right: u32,
        bottom: u32,
        words: Vec<String>,
    }

    let mut groups: BTreeMap<(u32, u32, u32, u32), LineBuilder> = BTreeMap::new();
    for row in tsv.lines().skip(1) {
        let fields: Vec<&str> = row.splitn(12, '\t').collect();
        if fields.len() != 12 || fields[0] != "5" {
            continue;
        }
        let parsed = (|| {
            Some((
                fields[1].parse::<u32>().ok()?,
                fields[2].parse::<u32>().ok()?,
                fields[3].parse::<u32>().ok()?,
                fields[4].parse::<u32>().ok()?,
                fields[6].parse::<u32>().ok()?,
                fields[7].parse::<u32>().ok()?,
                fields[8].parse::<u32>().ok()?,
                fields[9].parse::<u32>().ok()?,
            ))
        })();
        let Some((page, block, paragraph, line, x, y, width, height)) = parsed else {
            continue;
        };
        let text = fields[11].trim();
        if text.is_empty() {
            continue;
        }
        let entry = groups.entry((page, block, paragraph, line)).or_default();
        if entry.words.is_empty() {
            entry.left = x;
            entry.top = y;
            entry.right = x.saturating_add(width);
            entry.bottom = y.saturating_add(height);
        } else {
            entry.left = entry.left.min(x);
            entry.top = entry.top.min(y);
            entry.right = entry.right.max(x.saturating_add(width));
            entry.bottom = entry.bottom.max(y.saturating_add(height));
        }
        entry.words.push(text.to_string());
    }
    groups
        .into_values()
        .filter_map(|line| {
            Some(TsvLine {
                x: line.left,
                y: line.top,
                width: line.right.checked_sub(line.left)?,
                height: line.bottom.checked_sub(line.top)?,
                text: line.words.join(" "),
            })
        })
        .collect()
}

fn centered_slot_centers(
    image_width: u32,
    count: usize,
    anchors: &[(&TsvLine, FoundMatch)],
) -> Vec<f64> {
    if count == 1 {
        return vec![image_width as f64 / 2.0];
    }
    let offsets: Vec<f64> = (0..count)
        .map(|index| index as f64 - (count as f64 - 1.0) / 2.0)
        .collect();
    let anchor_x: Vec<f64> = anchors
        .iter()
        .map(|(line, _)| (line.x as f64 + line.width as f64 / 2.0) / image_width as f64)
        .collect();
    let mut candidates = vec![0.077];
    for x in &anchor_x {
        for offset in &offsets {
            if offset.abs() < f64::EPSILON {
                continue;
            }
            let spacing = (*x - 0.5) / offset;
            if (0.05..=0.13).contains(&spacing) {
                candidates.push(spacing);
            }
        }
    }
    let spacing = candidates
        .into_iter()
        .min_by(|a, b| {
            let score = |candidate: f64| {
                let grid: Vec<f64> = offsets
                    .iter()
                    .map(|offset| 0.5 + offset * candidate)
                    .collect();
                let mut used = Vec::new();
                let mut error = 0.0;
                for anchor in &anchor_x {
                    let (index, distance) = grid
                        .iter()
                        .enumerate()
                        .map(|(index, center)| (index, (anchor - center).abs()))
                        .min_by(|left, right| left.1.total_cmp(&right.1))
                        .unwrap_or((0, 1.0));
                    error += distance;
                    if used.contains(&index) {
                        error += 0.05;
                    }
                    used.push(index);
                }
                error
            };
            score(*a).total_cmp(&score(*b))
        })
        .unwrap_or(0.077);
    offsets
        .iter()
        .map(|offset| (0.5 + offset * spacing) * image_width as f64)
        .collect()
}

fn read_dynamic_layout(
    ocr: &OcrWorker,
    frame: &CapturedFrame,
    catalog: &[OverlayCatalogItem],
    expected_slots: &AtomicUsize,
    run_dir: Option<&Path>,
    attempt: usize,
    timings: &mut OverlayStageTimings,
) -> Result<LayoutRead, String> {
    let debug_dir = diagnostics_attempt_dir(run_dir, attempt);
    let frame_png = encode_frame(&frame.image)?;
    let started = Instant::now();
    let tsv = ocr
        .recognize(frame_png, OcrMode::SparseTsv)
        .map_err(|e| format!("ocr_unavailable: {e}"))?;
    timings.sparse_ocr_ms += elapsed_ms(started);
    if let Some(dir) = &debug_dir {
        let _ = std::fs::write(dir.join("sparse.tsv"), &tsv);
    }
    let lines = parse_tsv_lines(&tsv);
    let expected_slots = expected_slots.load(Ordering::Acquire);
    let matching_started = Instant::now();
    let slot_before = timings.slot_ocr_ms;
    let result = layout_from_lines(
        frame,
        &lines,
        catalog,
        expected_slots,
        debug_dir.as_deref(),
        |slot| {
            let started = Instant::now();
            let text = encode_crop(&frame.image, slot.title)
                .and_then(|png| ocr.recognize(png, OcrMode::SingleLine))
                .unwrap_or_default();
            timings.slot_ocr_ms += elapsed_ms(started);
            text
        },
    );
    timings.matching_ms += elapsed_ms(matching_started)
        .saturating_sub(timings.slot_ocr_ms.saturating_sub(slot_before));
    result.ok_or_else(|| "reward_row_not_detected: no relic reward title row was detected".into())
}

fn read_expected_layout(
    ocr: &OcrWorker,
    frame: &CapturedFrame,
    catalog: &[OverlayCatalogItem],
    expected_slots: usize,
    debug_dir: Option<&Path>,
    timings: &mut OverlayStageTimings,
) -> Result<LayoutRead, String> {
    let layout = expected_reward_layout(frame, expected_slots);
    let slots = &layout.slots;
    let mut matches = Vec::new();
    let mut text_log = Vec::new();
    for slot in slots {
        let png = encode_crop(&frame.image, slot.title)?;
        if let Some(dir) = debug_dir {
            let _ = std::fs::write(dir.join(format!("slot-{}.png", slot.index)), &png);
        }
        let started = Instant::now();
        let text = ocr
            .recognize(png, OcrMode::SingleLine)
            .map_err(|e| format!("ocr_unavailable: {e}"))?;
        timings.slot_ocr_ms += elapsed_ms(started);
        let started = Instant::now();
        let best = match_ocr_lines(&text, catalog)
            .into_iter()
            .max_by(|a, b| a.confidence.total_cmp(&b.confidence));
        timings.matching_ms += elapsed_ms(started);
        text_log.push(format!("slot {}: {:?}", slot.index, text.trim()));
        if let Some(found) = best {
            matches.push((slot.index, found));
        }
    }
    if let Some(dir) = debug_dir {
        let _ = std::fs::write(dir.join("fast-path.txt"), text_log.join("\n"));
        if let Ok(json) = serde_json::to_vec_pretty(&layout) {
            let _ = std::fs::write(dir.join("layout.json"), json);
        }
    }
    if matches.len() != expected_slots {
        return Err(format!(
            "catalog_match_incomplete: recognized {} of {expected_slots} expected reward names",
            matches.len()
        ));
    }
    Ok(LayoutRead { layout, matches })
}

fn centered_layout_is_complete(read: &LayoutRead) -> bool {
    let indices: Vec<usize> = read.matches.iter().map(|(index, _)| *index).collect();
    match read.layout.count {
        // Warframe centers two rewards in the inner positions of its four-card
        // row, and a solo reward in the middle position of its three-card row.
        4 => indices == [0, 1, 2, 3] || indices == [1, 2],
        3 => indices == [0, 1, 2] || indices == [1],
        _ => false,
    }
}

fn read_centered_reward_layout(
    ocr: &OcrWorker,
    frame: &CapturedFrame,
    catalog: &[OverlayCatalogItem],
    debug_dir: Option<&Path>,
    timings: &mut OverlayStageTimings,
) -> Result<LayoutRead, String> {
    let mut diagnostic = Vec::new();
    let mut crop_number = 0usize;
    let matching_started = Instant::now();
    let slot_before = timings.slot_ocr_ms;
    let result = layout_from_reward_header(frame, catalog, &mut diagnostic, &mut |slot| {
        let current_crop = crop_number;
        crop_number += 1;
        let png = match encode_crop(&frame.image, slot.title) {
            Ok(png) => png,
            Err(_) => return String::new(),
        };
        if let Some(dir) = debug_dir {
            let _ = std::fs::write(dir.join(format!("fast-slot-{current_crop}.png")), &png);
        }
        let started = Instant::now();
        let text = ocr.recognize(png, OcrMode::SingleLine).unwrap_or_default();
        timings.slot_ocr_ms += elapsed_ms(started);
        text
    });
    timings.matching_ms += elapsed_ms(matching_started)
        .saturating_sub(timings.slot_ocr_ms.saturating_sub(slot_before));
    if let Some(dir) = debug_dir {
        let _ = std::fs::write(dir.join("centered-fast-path.txt"), diagnostic.join("\n"));
    }
    result.filter(centered_layout_is_complete).ok_or_else(|| {
        "centered_reward_incomplete: fixed reward crops did not form a complete row".into()
    })
}

fn expected_reward_layout(frame: &CapturedFrame, expected_slots: usize) -> RewardLayout {
    let centers = centered_slot_centers(frame.width, expected_slots, &[]);
    let slots: Vec<RewardSlotRect> = centers
        .iter()
        .enumerate()
        .map(|(index, center)| RewardSlotRect {
            index,
            card: NormalizedRect {
                x: (center / frame.width as f64 - 0.072).max(0.0),
                y: 0.24,
                width: 0.144,
                height: 0.28,
            },
            title: NormalizedRect {
                x: (center / frame.width as f64 - 0.072).max(0.0),
                y: 0.40,
                width: 0.144,
                height: 0.13,
            },
        })
        .collect();
    RewardLayout {
        count: expected_slots,
        confidence: 1.0,
        slots,
    }
}

fn reward_header_visible(frame: &CapturedFrame, lines: &[TsvLine]) -> bool {
    let header = lines
        .iter()
        .filter(|line| line.y + line.height / 2 <= frame.height / 5)
        .map(|line| normalize(&line.text))
        .collect::<Vec<_>>()
        .join("");
    header.contains("fissure") && header.contains("reward")
}

fn reward_design_viewport(frame: &CapturedFrame) -> (f64, f64) {
    let design_height = (frame.width as f64 / WARFRAME_DESIGN_ASPECT).min(frame.height as f64);
    let top = (frame.height as f64 - design_height) / 2.0;
    (top, design_height)
}

fn reward_header_layout(frame: &CapturedFrame, count: usize) -> RewardLayout {
    let (design_top, design_height) = reward_design_viewport(frame);
    let spacing = REWARD_SLOT_SPACING_PER_HEIGHT * design_height / frame.width as f64;
    let width = REWARD_CARD_WIDTH_PER_HEIGHT * design_height / frame.width as f64;
    let card_y = (design_top + design_height * 0.22) / frame.height as f64;
    let card_height = design_height * 0.25 / frame.height as f64;
    let title_y = (design_top + design_height * 0.35) / frame.height as f64;
    let title_height = design_height * 0.11 / frame.height as f64;
    let offsets = (0..count).map(|index| index as f64 - (count as f64 - 1.0) / 2.0);
    let slots = offsets
        .enumerate()
        .map(|(index, offset)| {
            let center = 0.5 + offset * spacing;
            RewardSlotRect {
                index,
                card: NormalizedRect {
                    x: center - width / 2.0,
                    y: card_y,
                    width,
                    height: card_height,
                },
                title: NormalizedRect {
                    x: center - width / 2.0,
                    y: title_y,
                    width,
                    height: title_height,
                },
            }
        })
        .collect();
    RewardLayout {
        count,
        confidence: 0.0,
        slots,
    }
}

fn vertical_reward_edge_profile(frame: &CapturedFrame) -> Vec<f64> {
    let image = &frame.image;
    let mut profile = vec![0.0; image.width() as usize];
    let (design_top, design_height) = reward_design_viewport(frame);
    let top = (design_top + design_height * 0.20).round() as u32;
    let bottom = (design_top + design_height * 0.48).round() as u32;
    for x in 2..image.width().saturating_sub(2) {
        let mut strength = 0.0;
        for y in (top..bottom).step_by(2) {
            let left = image.get_pixel(x - 2, y).0;
            let right = image.get_pixel(x + 2, y).0;
            let left_luma = u32::from(left[0]) + u32::from(left[1]) + u32::from(left[2]);
            let right_luma = u32::from(right[0]) + u32::from(right[1]) + u32::from(right[2]);
            strength += left_luma.abs_diff(right_luma) as f64;
        }
        if let Some(value) = profile.get_mut(x as usize) {
            *value = strength;
        }
    }
    profile
}

fn local_edge_strength(profile: &[f64], x: f64) -> f64 {
    let center = x.round() as isize;
    (-3..=3)
        .filter_map(|offset| profile.get((center + offset).max(0) as usize))
        .copied()
        .fold(0.0, f64::max)
}

/// Recover the horizontal card grid without invoking OCR. Card borders form a
/// repeated set of long vertical edges, so a small projection over the reward
/// band can adjust the center and spacing before title crops are made.
fn detected_reward_header_layout(frame: &CapturedFrame, count: usize) -> Option<RewardLayout> {
    if !(3..=4).contains(&count) {
        return None;
    }
    let profile = vertical_reward_edge_profile(frame);
    let search_left = (frame.width as f64 * 0.18).round() as usize;
    let search_right = (frame.width as f64 * 0.82).round() as usize;
    let background = profile.get(search_left..search_right)?;
    let mean = background.iter().sum::<f64>() / background.len().max(1) as f64;
    let variance = background
        .iter()
        .map(|value| (value - mean).powi(2))
        .sum::<f64>()
        / background.len().max(1) as f64;
    let deviation = variance.sqrt().max(1.0);

    let (design_top, design_height) = reward_design_viewport(frame);
    let nominal_spacing = REWARD_SLOT_SPACING_PER_HEIGHT * design_height;
    let width_ratio = REWARD_CARD_WIDTH_PER_HEIGHT / REWARD_SLOT_SPACING_PER_HEIGHT;
    let step = (design_height / 600.0).round().max(1.0);
    let center = frame.width as f64 / 2.0;
    let mut best: Option<(f64, f64)> = None;
    let spacing_min = nominal_spacing * 0.82;
    let spacing_max = nominal_spacing * 1.18;
    let mut spacing = spacing_min;
    while spacing <= spacing_max {
        let width = spacing * width_ratio;
        let score = (0..count)
            .flat_map(|index| {
                let offset = index as f64 - (count as f64 - 1.0) / 2.0;
                let card_center = center + offset * spacing;
                [card_center - width / 2.0, card_center + width / 2.0]
            })
            .map(|edge| local_edge_strength(&profile, edge))
            .sum::<f64>()
            / (count * 2) as f64;
        if best.is_none_or(|(best_score, _)| score > best_score) {
            best = Some((score, spacing));
        }
        spacing += step;
    }
    let (score, spacing) = best?;
    let confidence = (score - mean) / deviation;
    if confidence < 0.8 {
        return None;
    }
    let width = spacing * width_ratio;
    let card_y = (design_top + design_height * 0.22) / frame.height as f64;
    let card_height = design_height * 0.25 / frame.height as f64;
    let title_y = (design_top + design_height * 0.35) / frame.height as f64;
    let title_height = design_height * 0.11 / frame.height as f64;
    let slots = (0..count)
        .map(|index| {
            let offset = index as f64 - (count as f64 - 1.0) / 2.0;
            let card_center = center + offset * spacing;
            let x = ((card_center - width / 2.0) / frame.width as f64).max(0.0);
            let normalized_width = (width / frame.width as f64).min(1.0 - x);
            RewardSlotRect {
                index,
                card: NormalizedRect {
                    x,
                    y: card_y,
                    width: normalized_width,
                    height: card_height,
                },
                title: NormalizedRect {
                    x,
                    y: title_y,
                    width: normalized_width,
                    height: title_height,
                },
            }
        })
        .collect();
    Some(RewardLayout {
        count,
        confidence,
        slots,
    })
}

fn layout_from_reward_header(
    frame: &CapturedFrame,
    catalog: &[OverlayCatalogItem],
    diagnostic: &mut Vec<String>,
    read_crop: &mut impl FnMut(&RewardSlotRect) -> String,
) -> Option<LayoutRead> {
    let mut best: Option<(usize, f64, LayoutRead)> = None;
    // Four-slot geometry also covers the centered two-slot row; three-slot
    // geometry likewise covers a centered solo reward.
    for count in [4, 3] {
        let mut layout = detected_reward_header_layout(frame, count)
            .unwrap_or_else(|| reward_header_layout(frame, count));
        diagnostic.push(format!(
            "header grid {count} geometry confidence={:.2}",
            layout.confidence
        ));
        let mut matches = Vec::new();
        let mut confidence_sum = 0.0;
        for slot in &layout.slots {
            let text = read_crop(slot);
            let found = match_ocr_lines(&text, catalog)
                .into_iter()
                .max_by(|a, b| a.confidence.total_cmp(&b.confidence));
            diagnostic.push(format!(
                "header grid {count} slot {} ocr={:?} match={:?}",
                slot.index,
                text.trim(),
                found
                    .as_ref()
                    .map(|candidate| (&candidate.item.name, candidate.confidence))
            ));
            if let Some(found) = found {
                confidence_sum += found.confidence;
                matches.push((slot.index, found));
            }
        }
        if matches.is_empty() {
            continue;
        }
        layout.confidence = matches.len() as f64 / count as f64;
        let candidate = LayoutRead { layout, matches };
        let score = (candidate.matches.len(), confidence_sum);
        if best
            .as_ref()
            .is_none_or(|(matches, confidence, _)| score > (*matches, *confidence))
        {
            best = Some((score.0, score.1, candidate));
        }
    }
    best.map(|(_, _, read)| read)
}

/// The post-OCR half of read_dynamic_layout: find the reward band from the
/// sparse-TSV lines, recover the slot grid, and match each slot crop against
/// the catalog. Pure (the crop reader is injected) so the corpus gate can feed
/// synthetic screens without a Tesseract engine.
#[allow(
    clippy::indexing_slicing,
    reason = "windows(2) pairs are exactly two elements; gaps is non-empty whenever the centers.len() > 1 branch runs"
)]
fn layout_from_lines(
    frame: &CapturedFrame,
    lines: &[TsvLine],
    catalog: &[OverlayCatalogItem],
    expected_slots: usize,
    debug_dir: Option<&std::path::Path>,
    mut read_crop: impl FnMut(&RewardSlotRect) -> String,
) -> Option<LayoutRead> {
    let mut diagnostic = Vec::new();
    let has_reward_header = reward_header_visible(frame, lines);
    let anchors: Vec<(&TsvLine, FoundMatch)> = lines
        .iter()
        .filter(|line| {
            let center_x = line.x as f64 + line.width as f64 / 2.0;
            let center_y = line.y as f64 + line.height as f64 / 2.0;
            center_x >= frame.width as f64 * 0.2
                && center_x <= frame.width as f64 * 0.8
                && center_y >= frame.height as f64 * 0.15
                && center_y <= frame.height as f64 * 0.72
        })
        .filter_map(|line| {
            match_ocr_lines(&line.text, catalog)
                .into_iter()
                .max_by(|a, b| a.confidence.total_cmp(&b.confidence))
                .map(|found| (line, found))
        })
        .collect();
    let anchor_y = anchors
        .iter()
        .map(|(line, _)| line.y + line.height / 2)
        .min();
    let Some(anchor_y) = anchor_y else {
        let result = has_reward_header
            .then(|| layout_from_reward_header(frame, catalog, &mut diagnostic, &mut read_crop))
            .flatten();
        if let Some(dir) = debug_dir {
            let _ = std::fs::write(dir.join("ocr.txt"), diagnostic.join("\n"));
        }
        return result;
    };
    let y_tolerance = (frame.height as f64 * 0.025).round() as u32;
    let mut row: Vec<&TsvLine> = lines
        .iter()
        .filter(|line| {
            let center_x = line.x as f64 + line.width as f64 / 2.0;
            let center_y = line.y + line.height / 2;
            let normalized = normalize(&line.text);
            center_x >= frame.width as f64 * 0.2
                && center_x <= frame.width as f64 * 0.8
                && center_y.abs_diff(anchor_y) <= y_tolerance
                && line.width >= frame.width / 40
                && line.width <= frame.width / 5
                && line.height >= frame.height / 200
                && line.height <= frame.height / 14
                && normalized.len() >= 6
        })
        .collect();
    row.sort_by_key(|line| line.x);
    row.dedup_by(|a, b| {
        let a_center = a.x + a.width / 2;
        let b_center = b.x + b.width / 2;
        a_center.abs_diff(b_center) < frame.width / 50
    });
    if row.is_empty() || (expected_slots == 0 && row.len() > 4) {
        diagnostic.push(format!("dynamic title row had {} candidates", row.len()));
        let result = has_reward_header
            .then(|| layout_from_reward_header(frame, catalog, &mut diagnostic, &mut read_crop))
            .flatten();
        if let Some(dir) = debug_dir {
            let _ = std::fs::write(dir.join("ocr.txt"), diagnostic.join("\n"));
        }
        return result;
    }

    let observed_centers: Vec<f64> = row
        .iter()
        .map(|line| line.x as f64 + line.width as f64 / 2.0)
        .collect();
    let count = if (1..=4).contains(&expected_slots) {
        expected_slots
    } else {
        observed_centers.len()
    };
    let centers: Vec<f64> = if (1..=4).contains(&expected_slots) {
        centered_slot_centers(frame.width, expected_slots, &anchors)
    } else {
        observed_centers.clone()
    };
    let card_width = if centers.len() > 1 {
        let mut gaps: Vec<f64> = centers.windows(2).map(|pair| pair[1] - pair[0]).collect();
        gaps.sort_by(|a, b| a.total_cmp(b));
        gaps[gaps.len() / 2] * 0.92
    } else {
        frame.width as f64 * 0.13
    }
    .clamp(frame.width as f64 * 0.065, frame.width as f64 * 0.18);
    let row_top = row.iter().map(|line| line.y).min()? as f64;
    let row_bottom = row.iter().map(|line| line.y + line.height).max()? as f64;
    let crop_top = (row_top - frame.height as f64 * 0.012).max(0.0);
    let crop_bottom = (row_bottom + frame.height as f64 * 0.012).min(frame.height as f64);
    let slots: Vec<RewardSlotRect> = centers
        .iter()
        .enumerate()
        .map(|(index, center)| {
            let title = NormalizedRect {
                x: ((center - card_width / 2.0) / frame.width as f64).max(0.0),
                y: crop_top / frame.height as f64,
                width: card_width / frame.width as f64,
                height: (crop_bottom - crop_top) / frame.height as f64,
            };
            RewardSlotRect {
                index,
                card: NormalizedRect {
                    x: title.x,
                    y: (title.y - 0.20).max(0.0),
                    width: title.width,
                    height: 0.28,
                },
                title,
            }
        })
        .collect();
    let mut matches = Vec::new();
    for slot in &slots {
        let png = encode_crop(&frame.image, slot.title).ok()?;
        if let Some(dir) = &debug_dir {
            let _ = std::fs::write(dir.join(format!("slot-{}.png", slot.index)), &png);
        }
        let text = read_crop(slot);
        let best = match_ocr_lines(&text, catalog)
            .into_iter()
            .max_by(|a, b| a.confidence.total_cmp(&b.confidence));
        diagnostic.push(format!(
            "slot {} ocr={:?} match={:?}",
            slot.index,
            text.trim(),
            best.as_ref()
                .map(|found| (&found.item.name, found.confidence))
        ));
        if let Some(found) = best {
            matches.push((slot.index, found));
        }
    }
    let required = if expected_slots > 0 {
        slots.len()
    } else {
        slots.len().min(2)
    };
    let confidence = matches.len() as f64 / slots.len() as f64;
    let layout = RewardLayout {
        count,
        confidence,
        slots,
    };
    if let Some(dir) = debug_dir {
        if let Ok(json) = serde_json::to_vec_pretty(&layout) {
            let _ = std::fs::write(dir.join("layout.json"), json);
        }
        let _ = std::fs::write(dir.join("ocr.txt"), diagnostic.join("\n"));
    }
    (matches.len() >= required).then_some(LayoutRead { layout, matches })
}

#[cfg(target_os = "windows")]
fn capture_warframe() -> Result<CapturedFrame, String> {
    let windows =
        xcap::Window::all().map_err(|e| format!("capture_failed: listing windows: {e}"))?;
    let window = windows
        .into_iter()
        .find(|window| {
            window
                .title()
                .ok()
                .is_some_and(|title| title.to_ascii_lowercase().contains("warframe"))
        })
        .ok_or_else(|| {
            if std::env::var_os("WAYLAND_DISPLAY").is_some() {
                "window_not_found: Warframe window not found - capture runs through XWayland for now; run Warframe borderless/windowed with XWayland enabled"
                    .to_string()
            } else {
                "window_not_found: Warframe window not found; use borderless or windowed mode".to_string()
            }
        })?;
    let x = window
        .x()
        .map_err(|e| format!("capture_failed: reading Warframe position: {e}"))?;
    let y = window
        .y()
        .map_err(|e| format!("capture_failed: reading Warframe position: {e}"))?;
    let image = window
        .capture_image()
        .map_err(|e| format!("capture_failed: capturing Warframe (capture may be denied): {e}"))?;
    let width = image.width();
    let height = image.height();
    if width < 640 || height < 360 {
        return Err(format!(
            "capture_failed: Warframe capture is too small ({width}×{height})"
        ));
    }
    Ok(CapturedFrame {
        image,
        x,
        y,
        width,
        height,
    })
}

#[cfg(target_os = "linux")]
#[allow(
    clippy::indexing_slicing,
    reason = "the reply length is checked against expected and chunks_exact guarantees each pixel has the validated 2, 3, or 4-byte width"
)]
fn capture_warframe() -> Result<CapturedFrame, String> {
    use xcb::{
        x::{
            Atom, Drawable, GetGeometry, GetImage, GetProperty, ImageFormat, ImageOrder,
            InternAtom, TranslateCoordinates, Window, ATOM_NONE, ATOM_STRING, ATOM_WM_NAME,
        },
        Connection,
    };

    fn atom(connection: &Connection, name: &[u8]) -> Result<Atom, String> {
        let cookie = connection.send_request(&InternAtom {
            only_if_exists: false,
            name,
        });
        connection
            .wait_for_reply(cookie)
            .map(|reply| reply.atom())
            .map_err(|error| format!("capture_failed: resolving X11 atom: {error}"))
    }

    fn property(
        connection: &Connection,
        window: Window,
        property: Atom,
        property_type: Atom,
    ) -> Result<xcb::x::GetPropertyReply, String> {
        let cookie = connection.send_request(&GetProperty {
            delete: false,
            window,
            property,
            r#type: property_type,
            long_offset: 0,
            long_length: 4096,
        });
        connection
            .wait_for_reply(cookie)
            .map_err(|error| format!("capture_failed: reading X11 window property: {error}"))
    }

    let (connection, _) = Connection::connect(None)
        .map_err(|error| format!("capture_failed: connecting to X11/XWayland: {error}"))?;
    let client_list = atom(&connection, b"_NET_CLIENT_LIST_STACKING")?;
    let net_wm_name = atom(&connection, b"_NET_WM_NAME")?;
    let utf8_string = atom(&connection, b"UTF8_STRING")?;

    let mut warframe = None;
    for screen in connection.get_setup().roots() {
        let reply = match property(&connection, screen.root(), client_list, ATOM_NONE) {
            Ok(reply) => reply,
            Err(_) => continue,
        };
        for &window in reply.value::<Window>().iter().rev() {
            let mut title = property(&connection, window, net_wm_name, utf8_string)
                .ok()
                .map(|reply| String::from_utf8_lossy(reply.value()).into_owned())
                .unwrap_or_default();
            if title.is_empty() {
                title = property(&connection, window, ATOM_WM_NAME, ATOM_STRING)
                    .ok()
                    .map(|reply| String::from_utf8_lossy(reply.value()).into_owned())
                    .unwrap_or_default();
            }
            if title.to_ascii_lowercase().contains("warframe") {
                warframe = Some(window);
                break;
            }
        }
        if warframe.is_some() {
            break;
        }
    }

    let window = warframe.ok_or_else(|| {
        if std::env::var_os("WAYLAND_DISPLAY").is_some() {
            "window_not_found: Warframe window not found - capture runs through XWayland for now; run Warframe borderless/windowed with XWayland enabled"
                .to_string()
        } else {
            "window_not_found: Warframe window not found; use borderless or windowed mode".to_string()
        }
    })?;

    let geometry_cookie = connection.send_request(&GetGeometry {
        drawable: Drawable::Window(window),
    });
    let geometry = connection
        .wait_for_reply(geometry_cookie)
        .map_err(|error| format!("capture_failed: reading Warframe geometry: {error}"))?;
    let width = u32::from(geometry.width());
    let height = u32::from(geometry.height());
    if width < 640 || height < 360 {
        return Err(format!(
            "capture_failed: Warframe capture is too small ({width}×{height})"
        ));
    }

    let position_cookie = connection.send_request(&TranslateCoordinates {
        src_window: window,
        dst_window: geometry.root(),
        src_x: 0,
        src_y: 0,
    });
    let position = connection
        .wait_for_reply(position_cookie)
        .map_err(|error| format!("capture_failed: reading Warframe position: {error}"))?;

    let image_cookie = connection.send_request(&GetImage {
        format: ImageFormat::ZPixmap,
        drawable: Drawable::Window(window),
        x: 0,
        y: 0,
        width: geometry.width(),
        height: geometry.height(),
        plane_mask: u32::MAX,
    });
    let reply = connection.wait_for_reply(image_cookie).map_err(|error| {
        format!("capture_failed: capturing Warframe (capture may be denied): {error}")
    })?;
    let format = connection
        .get_setup()
        .pixmap_formats()
        .iter()
        .find(|format| format.depth() == reply.depth())
        .ok_or_else(|| {
            format!(
                "capture_failed: unsupported X11 pixel depth {}",
                reply.depth()
            )
        })?;
    let bits_per_pixel = u32::from(format.bits_per_pixel());
    if !matches!(bits_per_pixel, 16 | 24 | 32) {
        return Err(format!(
            "capture_failed: unsupported X11 pixel format ({bits_per_pixel} bits per pixel)"
        ));
    }
    let bytes_per_pixel = (bits_per_pixel / 8) as usize;
    let expected = width as usize * height as usize * bytes_per_pixel;
    if reply.data().len() < expected {
        return Err("capture_failed: X11 returned an incomplete Warframe frame".into());
    }

    let lsb_first = connection.get_setup().bitmap_format_bit_order() == ImageOrder::LsbFirst;
    let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
    for pixel in reply.data()[..expected].chunks_exact(bytes_per_pixel) {
        let (red, green, blue) = if bits_per_pixel == 16 {
            let packed = if lsb_first {
                u16::from_le_bytes([pixel[0], pixel[1]])
            } else {
                u16::from_be_bytes([pixel[0], pixel[1]])
            };
            (
                (((packed >> 11) as u32) * 255 / 31) as u8,
                ((((packed >> 5) & 63) as u32) * 255 / 63) as u8,
                (((packed & 31) as u32) * 255 / 31) as u8,
            )
        } else if lsb_first {
            (pixel[2], pixel[1], pixel[0])
        } else {
            (pixel[0], pixel[1], pixel[2])
        };
        rgba.extend_from_slice(&[red, green, blue, 255]);
    }
    let image = RgbaImage::from_raw(width, height, rgba)
        .ok_or_else(|| "capture_failed: constructing the X11 Warframe frame".to_string())?;

    Ok(CapturedFrame {
        image,
        x: i32::from(position.dst_x()),
        y: i32::from(position.dst_y()),
        width,
        height,
    })
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
fn capture_warframe() -> Result<CapturedFrame, String> {
    Err("capture_failed: relic reward capture is unsupported on this platform".into())
}

fn compact_overlay_geometry(
    frame: &CapturedFrame,
    slots: &mut [RelicOverlaySlot],
) -> (i32, i32, u32, u32) {
    let left = (slots.iter().map(|slot| slot.box_.x).fold(1.0, f64::min) - 0.008).max(0.0);
    let right = (slots
        .iter()
        .map(|slot| slot.box_.x + slot.box_.width)
        .fold(0.0, f64::max)
        + 0.008)
        .min(1.0);
    let top = (slots.iter().map(|slot| slot.box_.y).fold(1.0, f64::min) - 0.005).max(0.0);
    let bottom = (slots
        .iter()
        .map(|slot| slot.box_.y + slot.box_.height)
        .fold(0.0, f64::max)
        + 0.015)
        .min(1.0);
    let span_x = (right - left).max(0.01);
    let span_y = (bottom - top).max(0.01);
    for slot in slots {
        slot.box_.x = (slot.box_.x - left) / span_x;
        slot.box_.y = (slot.box_.y - top) / span_y;
        slot.box_.width /= span_x;
        slot.box_.height /= span_y;
    }
    (
        frame.x + (left * frame.width as f64).floor() as i32,
        frame.y + (top * frame.height as f64).floor() as i32,
        (span_x * frame.width as f64).ceil().max(1.0) as u32,
        (span_y * frame.height as f64).ceil().max(1.0) as u32,
    )
}

/// Assemble recognized matches into positioned, fact-joined, best-marked
/// overlay slots plus the compact window geometry. Pure (market facts and the
/// owned map are injected) so the corpus gate can drive the full pipeline
/// without a MarketCache or DB.
#[allow(
    clippy::indexing_slicing,
    reason = "matches carry slot.index from the slots scan in layout_from_lines; index < slots.len() by construction"
)]
fn assemble_result(
    frame: &CapturedFrame,
    read: LayoutRead,
    settings: &OverlaySettings,
    market_facts: impl Fn(Option<&str>) -> OverlayMarketFacts,
    owned: Option<&HashMap<String, u32>>,
    capture_id: String,
) -> (RelicOverlayResult, (i32, i32, u32, u32)) {
    let panel_y = read
        .layout
        .slots
        .iter()
        .map(|slot| slot.card.y + slot.card.height)
        .fold(0.5, f64::max)
        + 0.035;
    let panel_y = panel_y.min(0.82);
    let layout = read.layout;
    let matches = read.matches;
    let mut slots: Vec<RelicOverlaySlot> = matches
        .into_iter()
        .map(|(index, found)| {
            let detected = &layout.slots[index];
            let facts = market_facts(found.item.slug.as_deref());
            RelicOverlaySlot {
                index,
                box_: OverlayBox {
                    x: detected.card.x,
                    y: panel_y,
                    width: detected.card.width,
                    height: 0.14,
                },
                raw_text: found.raw,
                name: Some(found.item.name),
                slug: found.item.slug.clone(),
                confidence: found.confidence,
                cached_platinum: facts.cached_platinum,
                live_platinum: None,
                ducats: facts.ducats,
                owned: match (owned, found.item.slug.as_deref()) {
                    (Some(totals), Some(slug)) => Some(*totals.get(slug).unwrap_or(&0)),
                    _ => None,
                },
                best_platinum: false,
                best_ducats: false,
            }
        })
        .collect();
    mark_bests(&mut slots);
    let overlay_geometry = compact_overlay_geometry(frame, &mut slots);
    let result = RelicOverlayResult {
        capture_id,
        captured_at: unix_millis().to_string(),
        scale: settings.scale,
        slots,
    };
    (result, overlay_geometry)
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
    let mut recognized: Option<(CapturedFrame, LayoutRead)> = None;
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
                        recognized = Some((frame, read));
                        break;
                    }
                    Err(error) => last_error = Some(error),
                }
                if recognized.is_none() && (1..=4).contains(&expected_slots) {
                    match read_expected_layout(
                        &state.ocr,
                        &frame,
                        &catalog,
                        expected_slots,
                        debug_dir.as_deref(),
                        &mut timings,
                    ) {
                        Ok(read) => {
                            recognized = Some((frame, read));
                            break;
                        }
                        Err(error) => last_error = Some(error),
                    }
                }
                // Full-frame sparse OCR is the compatibility fallback, not the
                // normal hot path. Run it once only after the inexpensive
                // centered crops have had time to catch a drawing transition.
                if recognized.is_none() && attempt == 2 {
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
                            recognized = Some((frame, read));
                            break;
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
        let error = format!(
            "{}; retry while the reward names are visible",
            last_error.unwrap_or_else(|| "reward recognition failed".into())
        );
        timings.total_ms = elapsed_ms(run_started);
        write_run_diagnostics(run_dir.as_deref(), &timings, None);
        state.finish_run(
            app,
            OverlayLastRun {
                outcome: error.clone(),
                trigger_source: source.into(),
                expected_slots,
                recognized_slots: 0,
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
        |slug| market.overlay_market_facts(slug),
        owned.as_ref(),
        capture_id.clone(),
    );
    *state
        .current_capture
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = Some(capture_id.clone());
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
        let queries: Vec<wfm_core::live_top::LiveTopQuery> = result
            .slots
            .iter()
            .filter_map(|slot| {
                slot.slug
                    .as_ref()
                    .map(|slug| wfm_core::live_top::LiveTopQuery {
                        slug: slug.clone(),
                        rank: None,
                        subtype: None,
                    })
            })
            .collect();
        if !queries.is_empty() {
            if let Ok(live) = wfm_core::live_top::fetch_live_tops("pc", None, &queries, |_, _| {}) {
                for slot in &mut result.slots {
                    slot.live_platinum = slot
                        .slug
                        .as_ref()
                        .and_then(|slug| live.iter().find(|row| &row.slug == slug))
                        .and_then(|row| row.low_sell);
                }
                mark_bests(&mut result.slots);
                let current = state
                    .current_capture
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .clone();
                if current.as_deref() == Some(&capture_id) {
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

    let app_for_hide = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(20));
        let state = app_for_hide.state::<OverlayState>();
        let current = state
            .current_capture
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        if current.as_deref() == Some(&capture_id) {
            hide_overlay(&app_for_hide);
            state.set_status(&app_for_hide, "watching", None);
        }
    });
    Ok(())
}

fn present_overlay(
    app: &AppHandle,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    result: &RelicOverlayResult,
) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        let geometry = crate::wayland_overlay::OverlayGeometry {
            x,
            y,
            width,
            height,
        };
        match crate::wayland_overlay::show(geometry, result) {
            Ok(()) => {
                if let Some(window) = app.get_webview_window("relic-overlay") {
                    let _ = window.hide();
                }
                if let Some(state) = app.try_state::<OverlayState>() {
                    state.set_presentation_backend(app, "wayland-layer-shell");
                }
                return Ok(());
            }
            Err(error) => {
                eprintln!("tennoworth: native Wayland overlay unavailable, using window fallback: {error}");
                if let Some(state) = app.try_state::<OverlayState>() {
                    state.set_presentation_backend(app, "tauri-window");
                }
            }
        }
    }
    show_tauri_overlay(app, x, y, width, height)?;
    push_tauri_overlay_result(app, result)
}

fn show_tauri_overlay(app: &AppHandle, x: i32, y: i32, width: u32, height: u32) -> Result<(), String> {
    let app_for_window = app.clone();
    let (reply_tx, reply_rx) = mpsc::sync_channel(1);
    app.run_on_main_thread(move || {
        let result = show_overlay_on_main_thread(&app_for_window, x, y, width, height);
        let _ = reply_tx.send(result);
    })
    .map_err(|e| format!("scheduling overlay window: {e}"))?;
    reply_rx
        .recv_timeout(Duration::from_secs(5))
        .map_err(|_| "timed out creating overlay window".to_string())?
}

fn show_overlay_on_main_thread(
    app: &AppHandle,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
) -> Result<(), String> {
    let window = ensure_overlay_window(app)?;
    window
        .set_always_on_top(true)
        .map_err(|e| format!("making overlay topmost: {e}"))?;
    window
        .set_focusable(false)
        .map_err(|e| format!("making overlay non-activating: {e}"))?;
    configure_linux_overlay_focus(&window)?;
    window
        .set_size(PhysicalSize::new(width, height))
        .map_err(|e| format!("sizing overlay: {e}"))?;
    window
        .set_position(PhysicalPosition::new(x, y))
        .map_err(|e| format!("positioning overlay: {e}"))?;
    window.show().map_err(|e| format!("showing overlay: {e}"))?;
    // A hidden topmost window can lose its z-band while a borderless game is
    // active. Reassert after mapping without activating or stealing focus.
    window
        .set_always_on_top(true)
        .map_err(|e| format!("raising overlay above the game: {e}"))?;
    window
        .set_ignore_cursor_events(true)
        .map_err(|e| format!("making overlay click-through: {e}"))?;
    Ok(())
}

fn ensure_overlay_window(app: &AppHandle) -> Result<tauri::WebviewWindow, String> {
    let window = match app.get_webview_window("relic-overlay") {
        Some(window) => window,
        None => WebviewWindowBuilder::new(
            app,
            "relic-overlay",
            WebviewUrl::App("index.html?surface=relic-overlay".into()),
        )
        .title("TennoWorth relic overlay")
        .transparent(true)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .focusable(false)
        .focused(false)
        .shadow(false)
        .resizable(false)
        .visible(false)
        .build()
        .map_err(|e| format!("creating overlay window: {e}"))?,
    };
    Ok(window)
}

fn push_overlay_result(app: &AppHandle, result: &RelicOverlayResult) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    if crate::wayland_overlay::is_active() && crate::wayland_overlay::update(result)? {
        return Ok(());
    }
    push_tauri_overlay_result(app, result)
}

fn push_tauri_overlay_result(app: &AppHandle, result: &RelicOverlayResult) -> Result<(), String> {
    let event_error = app
        .emit_to("relic-overlay", EVENT_UPDATE, result)
        .err()
        .map(|error| error.to_string());
    let eval_error = app
        .get_webview_window("relic-overlay")
        .ok_or_else(|| "overlay window is missing".to_string())
        .and_then(|window| {
            let payload = serde_json::to_string(result)
                .map_err(|error| format!("encoding overlay result: {error}"))?;
            window
                .eval(format!(
                    "window.__TENNOWORTH_RELIC_OVERLAY_UPDATE__?.({payload})"
                ))
                .map_err(|error| format!("updating overlay webview: {error}"))
        })
        .err();
    if let Some(eval_error) = eval_error {
        return Err(format!(
            "cached_overlay_display_failed: bridge={eval_error}; event={}",
            event_error.unwrap_or_else(|| "sent (listener delivery unverified)".into())
        ));
    }
    Ok(())
}

pub fn prewarm_overlay_window(app: &AppHandle) {
    #[cfg(target_os = "linux")]
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        if crate::wayland_overlay::available() {
            return;
        }
        if let Some(state) = app.try_state::<OverlayState>() {
            state.set_presentation_backend(app, "tauri-window");
        }
    }
    if let Err(error) = ensure_overlay_window(app) {
        eprintln!("tennoworth: could not prewarm relic overlay: {error}");
    }
}

#[cfg(target_os = "linux")]
fn configure_linux_overlay_focus(window: &tauri::WebviewWindow) -> Result<(), String> {
    use gtk::prelude::{GtkWindowExt, WidgetExt};

    let native = window
        .gtk_window()
        .map_err(|e| format!("accessing native overlay window: {e}"))?;
    native.set_accept_focus(false);
    native.set_focus_on_map(false);
    native.set_can_focus(false);
    native.set_type_hint(gtk::gdk::WindowTypeHint::Toolbar);
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn configure_linux_overlay_focus(_window: &tauri::WebviewWindow) -> Result<(), String> {
    Ok(())
}

fn hide_overlay(app: &AppHandle) {
    #[cfg(target_os = "linux")]
    crate::wayland_overlay::hide();
    if let Some(window) = app.get_webview_window("relic-overlay") {
        let _ = app.emit_to("relic-overlay", EVENT_HIDE, ());
        let _ = window.eval("window.__TENNOWORTH_RELIC_OVERLAY_HIDE__?.()");
        let _ = window.hide();
    }
    if let Some(state) = app.try_state::<OverlayState>() {
        *state
            .current_capture
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = None;
        *state
            .current_result
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = None;
    }
}

fn preferred_presentation_backend() -> &'static str {
    #[cfg(target_os = "linux")]
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        return "wayland-layer-shell";
    }
    "tauri-window"
}

#[derive(Debug)]
struct FoundMatch {
    raw: String,
    item: OverlayCatalogItem,
    confidence: f64,
}

fn normalize(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[allow(
    clippy::indexing_slicing,
    reason = "count <= 3.min(lines.len() - start) keeps start..start+count in bounds"
)]
fn match_ocr_lines(text: &str, catalog: &[OverlayCatalogItem]) -> Vec<FoundMatch> {
    let normalized_catalog: Vec<(String, &OverlayCatalogItem)> = catalog
        .iter()
        .map(|item| (normalize(&item.name), item))
        .filter(|(name, _)| name.len() >= 4 && !name.ends_with(" relic"))
        .collect();
    let mut found: Vec<FoundMatch> = Vec::new();
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|line| line.len() >= 2)
        .collect();
    let joined = lines.join(" ");
    let mut reads: Vec<String> = Vec::new();
    if joined.len() >= 4 {
        reads.push(joined);
    }
    for start in 0..lines.len() {
        for count in 1..=3.min(lines.len() - start) {
            let read = lines[start..start + count].join(" ");
            if read.len() >= 4 && !reads.contains(&read) {
                reads.push(read);
            }
        }
    }
    for raw in reads {
        let line = normalize(&raw);
        if line.is_empty() {
            continue;
        }
        let mut scores: Vec<(f64, &OverlayCatalogItem)> = normalized_catalog
            .iter()
            .map(|(candidate, item)| {
                let whole_score = if &line == candidate || line.contains(candidate) {
                    1.0
                } else {
                    strsim::normalized_levenshtein(&line, candidate)
                };
                let read_words: Vec<&str> = line.split_whitespace().collect();
                let candidate_words: Vec<&str> = candidate.split_whitespace().collect();
                let positional_score = if read_words.len() == candidate_words.len() {
                    read_words
                        .iter()
                        .zip(candidate_words.iter())
                        .map(|(read, candidate)| strsim::normalized_levenshtein(read, candidate))
                        .sum::<f64>()
                        / read_words.len().max(1) as f64
                } else {
                    0.0
                };
                let score = whole_score.max(positional_score);
                (score, *item)
            })
            .collect();
        scores.sort_by(|a, b| b.0.total_cmp(&a.0));
        let Some((best, item)) = scores.first().copied() else {
            continue;
        };
        let runner_up = scores.get(1).map(|row| row.0).unwrap_or(0.0);
        if best < 0.82 || (best < 1.0 && best - runner_up < 0.08) {
            continue;
        }
        let candidate = FoundMatch {
            raw,
            item: item.clone(),
            confidence: best,
        };
        if let Some(existing) = found
            .iter_mut()
            .find(|existing| existing.item.name.eq_ignore_ascii_case(&item.name))
        {
            if candidate.confidence > existing.confidence {
                *existing = candidate;
            }
        } else {
            found.push(candidate);
        }
        if found.len() == 4 {
            break;
        }
    }
    found
}

fn mark_bests(slots: &mut [RelicOverlaySlot]) {
    for slot in slots.iter_mut() {
        slot.best_platinum = false;
        slot.best_ducats = false;
    }
    let best_platinum = slots
        .iter()
        .filter(|slot| slot.confidence >= RECOMMENDATION_CONFIDENCE)
        .filter_map(|slot| slot.live_platinum.or(slot.cached_platinum))
        .max();
    let best_ducats = slots
        .iter()
        .filter(|slot| slot.confidence >= RECOMMENDATION_CONFIDENCE)
        .filter_map(|slot| slot.ducats)
        .max();
    for slot in slots {
        let recommendable = slot.confidence >= RECOMMENDATION_CONFIDENCE;
        slot.best_platinum = recommendable
            && best_platinum.is_some()
            && slot.live_platinum.or(slot.cached_platinum) == best_platinum;
        slot.best_ducats = recommendable && best_ducats.is_some() && slot.ducats == best_ducats;
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog() -> Vec<OverlayCatalogItem> {
        vec![
            OverlayCatalogItem {
                name: "Paris Prime Blueprint".into(),
                slug: Some("paris_prime_blueprint".into()),
            },
            OverlayCatalogItem {
                name: "Wisp Prime Systems Blueprint".into(),
                slug: Some("wisp_prime_systems_blueprint".into()),
            },
            OverlayCatalogItem {
                name: "Forma Blueprint".into(),
                slug: None,
            },
        ]
    }

    fn positioned_slot(index: usize, x: f64) -> RelicOverlaySlot {
        RelicOverlaySlot {
            index,
            box_: OverlayBox {
                x,
                y: 0.54,
                width: 0.07,
                height: 0.14,
            },
            raw_text: String::new(),
            name: None,
            slug: None,
            confidence: 1.0,
            cached_platinum: None,
            live_platinum: None,
            ducats: None,
            owned: None,
            best_platinum: false,
            best_ducats: false,
        }
    }

    #[test]
    fn exact_and_guarded_fuzzy_matches_resolve_but_noise_does_not() {
        let got = match_ocr_lines("SELECT A REWARD\nParis Prime Blueprint\nWisp Prime Systerns Blueprint\nrandom mission text", &catalog());
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].item.slug.as_deref(), Some("paris_prime_blueprint"));
        assert_eq!(got[0].confidence, 1.0);
        assert_eq!(got[1].item.name, "Wisp Prime Systems Blueprint");
        assert!(got[1].confidence >= 0.82 && got[1].confidence < 1.0);
    }

    #[test]
    fn relic_inventory_labels_are_not_reward_candidates() {
        let mut candidates = catalog();
        candidates.push(OverlayCatalogItem {
            name: "Axi V13 Relic".into(),
            slug: Some("axi_v13_relic".into()),
        });

        let got = match_ocr_lines(
            "VOID RELICS / REFINEMENT\nAxi V13 Relic\nAxi V14 Relic",
            &candidates,
        );
        assert!(got.is_empty());
    }

    #[test]
    fn untradeable_rewards_stay_recognized_without_a_slug() {
        let got = match_ocr_lines("Forma Blueprint", &catalog());
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].item.slug, None);
    }

    #[test]
    fn surrounding_crop_noise_does_not_downgrade_an_exact_reward_phrase() {
        let got = match_ocr_lines("aS i@\nWisp Prime Systems Blueprint\n-", &catalog());
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].item.name, "Wisp Prime Systems Blueprint");
        assert_eq!(got[0].confidence, 1.0);
    }

    #[test]
    fn wrapped_reward_title_tolerates_one_bad_word() {
        let mut candidates = catalog();
        candidates.extend([
            OverlayCatalogItem {
                name: "Grendel Prime Chassis Blueprint".into(),
                slug: Some("grendel_prime_chassis_blueprint".into()),
            },
            OverlayCatalogItem {
                name: "Grendel Prime Systems Blueprint".into(),
                slug: Some("grendel_prime_systems_blueprint".into()),
            },
        ]);
        let got = match_ocr_lines(
            "screen noise\nGrendel Brin Chassis\nBlueprint\nmore noise",
            &candidates,
        );
        let grendel = got
            .iter()
            .find(|found| found.item.name == "Grendel Prime Chassis Blueprint")
            .expect("wrapped title should match the chassis reward");
        assert!(grendel.confidence >= 0.82);
    }

    #[test]
    fn tsv_words_are_grouped_into_positioned_lines() {
        let tsv = "level\tpage_num\tblock_num\tpar_num\tline_num\tword_num\tleft\ttop\twidth\theight\tconf\ttext\n\
5\t1\t51\t1\t1\t1\t1313\t637\t35\t14\t87.7\tCedo\n\
5\t1\t51\t1\t1\t2\t1354\t638\t41\t13\t95.2\tPrime\n\
5\t1\t51\t1\t1\t3\t1400\t637\t40\t14\t94.8\tBarrel\n\
5\t1\t52\t1\t1\t1\t1514\t638\t45\t13\t97.0\tForma\n\
5\t1\t52\t1\t1\t2\t1564\t637\t64\t18\t96.8\tBlueprint";
        let lines = parse_tsv_lines(tsv);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].text, "Cedo Prime Barrel");
        assert_eq!(
            (lines[0].x, lines[0].y, lines[0].width, lines[0].height),
            (1313, 637, 127, 14)
        );
        assert_eq!(lines[1].text, "Forma Blueprint");
    }

    #[test]
    fn one_outer_anchor_recovers_a_centered_four_slot_grid() {
        let line = TsvLine {
            x: 1501,
            y: 627,
            width: 139,
            height: 24,
            text: "Braton Prime Barrel".into(),
        };
        let found = FoundMatch {
            raw: line.text.clone(),
            item: OverlayCatalogItem {
                name: line.text.clone(),
                slug: Some("braton_prime_barrel".into()),
            },
            confidence: 1.0,
        };
        let anchors = vec![(&line, found)];
        let centers = centered_slot_centers(2560, 4, &anchors);
        assert_eq!(centers.len(), 4);
        assert!((centers[0] - 989.5).abs() < 2.0);
        assert!((centers[3] - 1570.5).abs() < 2.0);
    }

    #[test]
    fn expected_slot_fast_path_uses_a_normalized_centered_reward_band() {
        let frame = corpus_frame();
        for count in 1..=4 {
            let layout = expected_reward_layout(&frame, count);
            assert_eq!(layout.count, count);
            assert_eq!(layout.slots.len(), count);
            assert!(layout
                .slots
                .iter()
                .all(|slot| slot.title.y == 0.40 && slot.title.height == 0.13));
            let average_center = layout
                .slots
                .iter()
                .map(|slot| slot.title.x + slot.title.width / 2.0)
                .sum::<f64>()
                / count as f64;
            assert!((average_center - 0.5).abs() < 0.001);
        }
    }

    #[test]
    fn reward_grid_uses_viewport_height_across_aspect_ratios() {
        let frame_16_9 = CapturedFrame {
            image: RgbaImage::new(2560, 1440),
            x: 0,
            y: 0,
            width: 2560,
            height: 1440,
        };
        let frame_ultrawide = CapturedFrame {
            image: RgbaImage::new(3440, 1440),
            x: 0,
            y: 0,
            width: 3440,
            height: 1440,
        };
        let frame_narrow = CapturedFrame {
            image: RgbaImage::new(795, 632),
            x: 0,
            y: 0,
            width: 795,
            height: 632,
        };
        let spacing = |layout: &RewardLayout, width: u32| {
            let center = |index: usize| {
                let title = layout.slots[index].title;
                (title.x + title.width / 2.0) * width as f64
            };
            center(1) - center(0)
        };
        let standard = reward_header_layout(&frame_16_9, 4);
        let ultrawide = reward_header_layout(&frame_ultrawide, 4);
        let narrow = reward_header_layout(&frame_narrow, 4);
        assert!((spacing(&standard, 2560) - spacing(&ultrawide, 3440)).abs() < 0.01);
        assert!((spacing(&standard, 2560) - 1440.0 * REWARD_SLOT_SPACING_PER_HEIGHT).abs() < 0.01);
        let narrow_design_height = 795.0 / WARFRAME_DESIGN_ASPECT;
        assert!(
            (spacing(&narrow, 795) - narrow_design_height * REWARD_SLOT_SPACING_PER_HEIGHT).abs()
                < 0.01
        );
        let narrow_title_top = narrow.slots[0].title.y * frame_narrow.height as f64;
        assert!((narrow_title_top - 249.0).abs() < 1.0);
    }

    #[test]
    fn reward_title_crops_are_normalized_for_ocr() {
        let image = RgbaImage::new(512, 100);
        let png = encode_crop(
            &image,
            NormalizedRect {
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 1.0,
            },
        )
        .unwrap();
        let decoded = image::load_from_memory(&png).unwrap();
        assert_eq!(decoded.width(), REWARD_TITLE_TARGET_WIDTH);
        assert_eq!(decoded.height(), 50);
    }

    #[test]
    fn repeated_card_edges_recover_a_shifted_reward_grid() {
        let mut image = RgbaImage::new(2560, 1440);
        let center = 1280.0;
        let spacing = 330.0;
        let width = spacing * REWARD_CARD_WIDTH_PER_HEIGHT / REWARD_SLOT_SPACING_PER_HEIGHT;
        for index in 0..4 {
            let offset = index as f64 - 1.5;
            let card_center = center + offset * spacing;
            for edge in [card_center - width / 2.0, card_center + width / 2.0] {
                let x = edge.round() as u32;
                for y in 288..692 {
                    image.put_pixel(x, y, image::Rgba([220, 220, 220, 255]));
                }
            }
        }
        let frame = CapturedFrame {
            image,
            x: 0,
            y: 0,
            width: 2560,
            height: 1440,
        };
        let layout = detected_reward_header_layout(&frame, 4).expect("detect reward grid");
        let centers: Vec<f64> = layout
            .slots
            .iter()
            .map(|slot| (slot.title.x + slot.title.width / 2.0) * frame.width as f64)
            .collect();
        assert!((centers[0] - (center - 1.5 * spacing)).abs() < 5.0);
        assert!((centers[3] - (center + 1.5 * spacing)).abs() < 5.0);
    }

    #[test]
    fn fragmented_reward_header_falls_back_to_centered_title_crops() {
        let frame = CapturedFrame {
            image: RgbaImage::new(1898, 1024),
            x: 0,
            y: 0,
            width: 1898,
            height: 1024,
        };
        let lines = vec![TsvLine {
            x: 388,
            y: 66,
            width: 372,
            height: 52,
            text: "FISSURE/REWARDS".into(),
        }];
        let candidates = vec![
            OverlayCatalogItem {
                name: "Wisp Prime Neuroptics Blueprint".into(),
                slug: Some("wisp_prime_neuroptics_blueprint".into()),
            },
            OverlayCatalogItem {
                name: "Lavos Prime Chassis Blueprint".into(),
                slug: Some("lavos_prime_chassis_blueprint".into()),
            },
            OverlayCatalogItem {
                name: "Protea Prime Chassis Blueprint".into(),
                slug: Some("protea_prime_chassis_blueprint".into()),
            },
            OverlayCatalogItem {
                name: "Dethcube Prime Cerebrum".into(),
                slug: Some("dethcube_prime_cerebrum".into()),
            },
        ];
        let names = [
            "Wisp Prime Neuroptics Blueprint",
            "Lavos Prime Chassis Blueprint",
            "Protea Prime Chassis Blueprint",
            "Dethcube Prime Cerebrum",
        ];
        let read = layout_from_lines(&frame, &lines, &candidates, 0, None, |slot| {
            let center = slot.title.x + slot.title.width / 2.0;
            [0.3215, 0.4405, 0.5595, 0.6785]
                .iter()
                .position(|expected| (center - expected).abs() < 0.001)
                .map(|index| names[index].to_string())
                .unwrap_or_else(|| "screen noise".into())
        })
        .expect("the reward header should activate centered crop fallback");
        assert_eq!(read.layout.count, 4);
        assert_eq!(read.matches.len(), 4);
    }

    #[test]
    fn centered_crop_fallback_requires_the_reward_header() {
        let frame = CapturedFrame {
            image: RgbaImage::new(1898, 1024),
            x: 0,
            y: 0,
            width: 1898,
            height: 1024,
        };
        let lines = vec![TsvLine {
            x: 388,
            y: 66,
            width: 372,
            height: 52,
            text: "MISSION COMPLETE".into(),
        }];
        assert!(layout_from_lines(&frame, &lines, &catalog(), 0, None, |_| {
            "Paris Prime Blueprint".into()
        })
        .is_none());
    }

    #[test]
    fn overlay_window_is_compact_and_slot_positions_become_window_relative() {
        let image = RgbaImage::new(2560, 1440);
        let frame = CapturedFrame {
            image,
            x: 0,
            y: 0,
            width: 2560,
            height: 1440,
        };
        let mut slots = vec![positioned_slot(0, 0.35), positioned_slot(3, 0.58)];
        let (x, y, width, height) = compact_overlay_geometry(&frame, &mut slots);
        assert_eq!((x, y), (875, 770));
        assert!(width < 900 && height < 240);
        assert!(slots[0].box_.x < 0.05);
        assert!(slots[1].box_.x > 0.7);
    }

    #[test]
    fn best_marks_use_live_over_cached_and_keep_ties() {
        let mut slots = vec![
            RelicOverlaySlot {
                index: 0,
                box_: OverlayBox {
                    x: 0.0,
                    y: 0.0,
                    width: 0.5,
                    height: 1.0,
                },
                raw_text: "a".into(),
                name: Some("a".into()),
                slug: Some("a".into()),
                confidence: 1.0,
                cached_platinum: Some(50),
                live_platinum: Some(20),
                ducats: Some(100),
                owned: Some(1),
                best_platinum: false,
                best_ducats: false,
            },
            RelicOverlaySlot {
                index: 1,
                box_: OverlayBox {
                    x: 0.5,
                    y: 0.0,
                    width: 0.5,
                    height: 1.0,
                },
                raw_text: "b".into(),
                name: Some("b".into()),
                slug: Some("b".into()),
                confidence: 1.0,
                cached_platinum: Some(30),
                live_platinum: None,
                ducats: Some(100),
                owned: Some(0),
                best_platinum: false,
                best_ducats: false,
            },
        ];
        mark_bests(&mut slots);
        assert!(!slots[0].best_platinum);
        assert!(slots[1].best_platinum);
        assert!(slots.iter().all(|slot| slot.best_ducats));
    }

    #[test]
    fn low_confidence_matches_never_receive_a_recommendation() {
        let mut slots = vec![
            RelicOverlaySlot {
                index: 0,
                box_: OverlayBox {
                    x: 0.0,
                    y: 0.0,
                    width: 0.5,
                    height: 1.0,
                },
                raw_text: "a".into(),
                name: Some("a".into()),
                slug: Some("a".into()),
                confidence: 0.89,
                cached_platinum: Some(500),
                live_platinum: None,
                ducats: Some(100),
                owned: None,
                best_platinum: false,
                best_ducats: false,
            },
            RelicOverlaySlot {
                index: 1,
                box_: OverlayBox {
                    x: 0.5,
                    y: 0.0,
                    width: 0.5,
                    height: 1.0,
                },
                raw_text: "b".into(),
                name: Some("b".into()),
                slug: Some("b".into()),
                confidence: 1.0,
                cached_platinum: Some(10),
                live_platinum: None,
                ducats: Some(15),
                owned: None,
                best_platinum: false,
                best_ducats: false,
            },
        ];
        mark_bests(&mut slots);
        assert!(!slots[0].best_platinum && !slots[0].best_ducats);
        assert!(slots[1].best_platinum && slots[1].best_ducats);
    }

    #[test]
    fn reward_marker_updates_are_bounded_and_trimmed() {
        install_reward_markers(&["  Rewards ready  ".into()]).unwrap();
        assert_eq!(&*REWARD_MARKERS.read().unwrap(), &["Rewards ready"]);
        assert!(install_reward_markers(&["".into()]).is_err());
        install_reward_markers(&[DEFAULT_REWARD_MARKER.into()]).unwrap();
    }

    #[test]
    fn recent_log_snapshot_finds_only_an_active_reward_batch() {
        let active = "1.0 Script [Info]: ProjectionRewardChoice.lua: Got rewards\n\
1.1 Script [Info]: ProjectionRewardChoice.lua: Missing icon data!\n\
1.2 Script [Info]: ProjectionRewardChoice.lua: Missing icon data!\n\
1.3 Script [Info]: ProjectionRewardChoice.lua: Missing icon data!\n\
1.4 Script [Info]: ProjectionRewardChoice.lua: Missing icon data!\n";
        let batch = latest_active_reward_batch(active).unwrap();
        assert!(batch.0.contains("Got rewards"));
        assert_eq!(batch.1, 4);

        let closed = format!("{active}2.0 Script [Info]: {REWARD_CLOSE_MARKER}\n");
        assert!(latest_active_reward_batch(&closed).is_none());
    }

    #[test]
    fn shared_result_fixture_obeys_the_recommendation_contract() {
        let raw = include_str!("../../../tests/fixtures/relic-ocr/result.json");
        let expected: RelicOverlayResult = serde_json::from_str(raw).unwrap();
        let expected_marks: Vec<(bool, bool)> = expected
            .slots
            .iter()
            .map(|slot| (slot.best_platinum, slot.best_ducats))
            .collect();
        let mut actual = expected.slots;
        mark_bests(&mut actual);
        assert_eq!(
            actual
                .iter()
                .map(|slot| (slot.best_platinum, slot.best_ducats))
                .collect::<Vec<_>>(),
            expected_marks
        );
    }

    #[test]
    fn settings_validation_clamps_scale_and_refuses_empty_shortcuts() {
        assert!(!OverlaySettings::default().diagnostics);
        let settings = OverlaySettings {
            scale: 9.0,
            ..OverlaySettings::default()
        };
        assert_eq!(validate_settings(settings).unwrap().scale, 1.5);
        let settings = OverlaySettings {
            shortcut: "   ".into(),
            ..OverlaySettings::default()
        };
        assert!(validate_settings(settings).is_err());
    }

    #[test]
    fn diagnostics_retention_keeps_the_newest_ten_runs() {
        let root = std::env::temp_dir().join(format!(
            "tennoworth-overlay-retention-{}-{}",
            std::process::id(),
            unix_millis()
        ));
        for index in 0..12 {
            create_diagnostics_run(&root, &format!("{index:02}")).expect("create diagnostics run");
        }
        let mut names: Vec<_> = std::fs::read_dir(&root)
            .expect("read diagnostics root")
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(
            names,
            (2..12)
                .map(|index| format!("{index:02}"))
                .collect::<Vec<_>>()
        );
        std::fs::remove_dir_all(root).expect("clean diagnostics fixture");
    }

    // ---- corpus gate: the plan's acceptance criteria, driven end to end ----
    //
    // ">=95% correct visible-slot recognition" and "0 wrong best-pick marks in
    // the gate corpus": synthetic reward screens run through the REAL pipeline
    // (parse_tsv_lines -> layout_from_lines -> assemble_result) with the OCR
    // seams injected, and every recognized slot and best-pick mark is asserted.

    fn corpus_frame() -> CapturedFrame {
        CapturedFrame {
            image: RgbaImage::new(2560, 1440),
            x: 0,
            y: 0,
            width: 2560,
            height: 1440,
        }
    }

    /// A reward band: one line per name, left to right, centered in the band
    /// the pipeline filters on, with mission/HUD noise around it.
    fn corpus_screen(names: &[&str]) -> Vec<TsvLine> {
        let n = names.len().max(1);
        let spacing = 2560.0 / (n as f64 + 1.0);
        let mut lines = vec![
            TsvLine {
                x: 200,
                y: 120,
                width: 300,
                height: 20,
                text: "VOID FISSURE COMPLETE".into(),
            },
            TsvLine {
                x: 300,
                y: 900,
                width: 260,
                height: 18,
                text: "Mission reward objective text".into(),
            },
        ];
        for (i, name) in names.iter().enumerate() {
            lines.push(TsvLine {
                x: (spacing * (i as f64 + 1.0) - 100.0).round() as u32,
                y: 708,
                width: 200,
                height: 24,
                text: name.to_string(),
            });
        }
        lines
    }

    fn corpus_catalog() -> Vec<OverlayCatalogItem> {
        vec![
            OverlayCatalogItem {
                name: "Paris Prime Blueprint".into(),
                slug: Some("paris_prime_blueprint".into()),
            },
            OverlayCatalogItem {
                name: "Wisp Prime Systems Blueprint".into(),
                slug: Some("wisp_prime_systems_blueprint".into()),
            },
            OverlayCatalogItem {
                name: "Cedo Prime Barrel".into(),
                slug: Some("cedo_prime_barrel".into()),
            },
            OverlayCatalogItem {
                name: "Forma Blueprint".into(),
                slug: None,
            },
        ]
    }

    fn corpus_facts(
        platinum: &[(&str, u32)],
        ducats: &[(&str, u32)],
    ) -> impl Fn(Option<&str>) -> OverlayMarketFacts {
        let plat: HashMap<String, u32> =
            platinum.iter().map(|(s, p)| (s.to_string(), *p)).collect();
        let duc: HashMap<String, u32> = ducats.iter().map(|(s, d)| (s.to_string(), *d)).collect();
        move |slug| OverlayMarketFacts {
            cached_platinum: slug.and_then(|s| plat.get(s).copied()),
            ducats: slug.and_then(|s| duc.get(s).copied()),
        }
    }

    #[test]
    fn corpus_four_reward_screen_recognizes_all_slots_and_marks_only_the_best() {
        let names = [
            "Paris Prime Blueprint",
            "Wisp Prime Systems Blueprint",
            "Cedo Prime Barrel",
            "Forma Blueprint",
        ];
        let frame = corpus_frame();
        let screen = corpus_screen(&names);
        let read = layout_from_lines(&frame, &screen, &corpus_catalog(), 4, None, |slot| {
            names[slot.index].to_string()
        })
        .expect("a four-reward screen must be detected");
        assert_eq!(read.layout.slots.len(), 4);
        assert_eq!(read.matches.len(), 4);
        assert!(read
            .matches
            .iter()
            .all(|(_, found)| found.confidence == 1.0));

        let (result, _) = assemble_result(
            &frame,
            read,
            &OverlaySettings::default(),
            corpus_facts(
                &[
                    ("paris_prime_blueprint", 15),
                    ("wisp_prime_systems_blueprint", 60),
                    ("cedo_prime_barrel", 30),
                ],
                &[
                    ("paris_prime_blueprint", 45),
                    ("wisp_prime_systems_blueprint", 100),
                    ("cedo_prime_barrel", 100),
                ],
            ),
            None,
            "corpus-1".into(),
        );
        assert_eq!(result.slots.len(), 4);
        assert!(result.slots.iter().all(|slot| slot.name.is_some()));

        let best_plat: Vec<&str> = result
            .slots
            .iter()
            .filter(|slot| slot.best_platinum)
            .map(|slot| slot.name.as_deref().unwrap())
            .collect();
        assert_eq!(best_plat, vec!["Wisp Prime Systems Blueprint"]);

        // Ducat tie (100/100) is the policy, not a miss: both are marked.
        let best_duc: Vec<&str> = result
            .slots
            .iter()
            .filter(|slot| slot.best_ducats)
            .map(|slot| slot.name.as_deref().unwrap())
            .collect();
        assert_eq!(best_duc.len(), 2);
        assert!(best_duc.contains(&"Wisp Prime Systems Blueprint"));
        assert!(best_duc.contains(&"Cedo Prime Barrel"));

        // Untradeable: recognized with no facts, never recommended.
        let forma = result
            .slots
            .iter()
            .find(|slot| slot.name.as_deref() == Some("Forma Blueprint"))
            .unwrap();
        assert!(forma.slug.is_none());
        assert!(forma.cached_platinum.is_none());
        assert!(!forma.best_platinum && !forma.best_ducats);
    }

    #[test]
    fn corpus_garbled_expensive_reward_is_shown_but_never_recommended() {
        // OCR reads "Wlsp Prime Systern Bluepint" -> fuzzy match ~0.875, below
        // RECOMMENDATION_CONFIDENCE. It still resolves to the 60p item, but
        // the best-pick mark must stay off even though it is the dearest.
        let names = [
            "Paris Prime Blueprint",
            "Wlsp Prime Systern Bluepint",
            "Cedo Prime Barrel",
            "Forma Blueprint",
        ];
        let frame = corpus_frame();
        let screen = corpus_screen(&names);
        let read = layout_from_lines(&frame, &screen, &corpus_catalog(), 4, None, |slot| {
            names[slot.index].to_string()
        })
        .expect("a four-reward screen must be detected");
        let (result, _) = assemble_result(
            &frame,
            read,
            &OverlaySettings::default(),
            corpus_facts(
                &[
                    ("paris_prime_blueprint", 15),
                    ("wisp_prime_systems_blueprint", 60),
                    ("cedo_prime_barrel", 30),
                ],
                &[],
            ),
            None,
            "corpus-2".into(),
        );
        let wisp = result
            .slots
            .iter()
            .find(|slot| slot.name.as_deref() == Some("Wisp Prime Systems Blueprint"))
            .expect("the garbled reward must still resolve by name");
        assert!(wisp.confidence < RECOMMENDATION_CONFIDENCE);
        assert_eq!(wisp.cached_platinum, Some(60));
        assert!(
            !wisp.best_platinum,
            "a low-confidence match must never be recommended"
        );
        let best_plat: Vec<&str> = result
            .slots
            .iter()
            .filter(|slot| slot.best_platinum)
            .map(|slot| slot.name.as_deref().unwrap())
            .collect();
        // The dearest is the garbled 60p wisp; ruled out, the mark falls to
        // the next recommendable slot.
        assert_eq!(
            best_plat,
            vec!["Cedo Prime Barrel"],
            "wrong best mark: {best_plat:?}"
        );
    }

    #[test]
    fn corpus_no_reward_screen_is_not_detected() {
        let frame = corpus_frame();
        let screen = vec![
            TsvLine {
                x: 300,
                y: 300,
                width: 400,
                height: 24,
                text: "Mission reward objective text".into(),
            },
            TsvLine {
                x: 500,
                y: 700,
                width: 300,
                height: 20,
                text: "Extraction complete".into(),
            },
        ];
        let read = layout_from_lines(&frame, &screen, &corpus_catalog(), 0, None, |_| {
            String::new()
        });
        assert!(
            read.is_none(),
            "no catalog match must mean no reward screen"
        );
    }

    #[test]
    fn corpus_two_reward_screen_detects_both() {
        let names = ["Paris Prime Blueprint", "Wisp Prime Systems Blueprint"];
        let frame = corpus_frame();
        let screen = corpus_screen(&names);
        let read = layout_from_lines(&frame, &screen, &corpus_catalog(), 2, None, |slot| {
            names[slot.index].to_string()
        })
        .expect("a two-reward screen must be detected");
        assert_eq!(read.layout.slots.len(), 2);
        assert_eq!(read.matches.len(), 2);
    }
}
