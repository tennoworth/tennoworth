//! Native Wayland presentation for the relic reward overlay.
//!
//! A regular `xdg_toplevel` cannot promise stacking or non-activation under
//! Wayland. This backend uses layer-shell for those compositor-owned semantics
//! and keeps the existing Tauri window as the compatibility fallback.

use std::ffi::CString;
use std::fs::File;
use std::os::fd::AsFd;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, LazyLock, Mutex};
use std::time::Duration;

use cairo::{Context, FontSlant, FontWeight, Format, ImageSurface, Operator};
use memmap2::MmapMut;
use rustix::event::{poll, PollFd, PollFlags};
use rustix::fs::{ftruncate, memfd_create, MemfdFlags};
use rustix::time::Timespec;
use wayland_client::globals::{registry_queue_init, GlobalListContents};
use wayland_client::protocol::{
    wl_buffer::{self, WlBuffer},
    wl_compositor::WlCompositor,
    wl_output::{self, WlOutput},
    wl_region::WlRegion,
    wl_registry::{self, WlRegistry},
    wl_shm::{self, WlShm},
    wl_shm_pool::WlShmPool,
    wl_surface::WlSurface,
};
use wayland_client::{delegate_noop, Connection, Dispatch, EventQueue, Proxy, QueueHandle, WEnum};
use wayland_protocols::wp::fractional_scale::v1::client::{
    wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1,
    wp_fractional_scale_v1::{self, WpFractionalScaleV1},
};
use wayland_protocols::wp::viewporter::client::{
    wp_viewport::WpViewport, wp_viewporter::WpViewporter,
};
use wayland_protocols::xdg::xdg_output::zv1::client::{
    zxdg_output_manager_v1::ZxdgOutputManagerV1,
    zxdg_output_v1::{self, ZxdgOutputV1},
};
use wayland_protocols_wlr::layer_shell::v1::client::{
    zwlr_layer_shell_v1::{Layer, ZwlrLayerShellV1},
    zwlr_layer_surface_v1::{self, Anchor, KeyboardInteractivity, ZwlrLayerSurfaceV1},
};

use crate::overlay::RelicOverlayResult;

const COMMAND_TIMEOUT: Duration = Duration::from_secs(3);
const BUFFER_COUNT: usize = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OverlayGeometry {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct XOutput {
    name: String,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

enum Command {
    Show {
        geometry: OverlayGeometry,
        x_output: Option<XOutput>,
        result: RelicOverlayResult,
        reply: mpsc::SyncSender<Result<(), String>>,
    },
    Update {
        result: RelicOverlayResult,
        reply: mpsc::SyncSender<Result<(), String>>,
    },
    Hide,
}

struct ClientHandle {
    tx: mpsc::Sender<Command>,
    active: Arc<AtomicBool>,
}

static CLIENT: LazyLock<Mutex<Option<ClientHandle>>> = LazyLock::new(|| Mutex::new(None));

pub fn available() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some() && ensure_client().is_ok()
}

pub fn show(geometry: OverlayGeometry, result: &RelicOverlayResult) -> Result<(), String> {
    ensure_client()?;
    let guard = CLIENT.lock().unwrap_or_else(|error| error.into_inner());
    let client = guard
        .as_ref()
        .ok_or_else(|| "Wayland overlay worker is unavailable".to_string())?;
    let (reply_tx, reply_rx) = mpsc::sync_channel(1);
    client
        .tx
        .send(Command::Show {
            geometry,
            x_output: x_output_for_geometry(geometry),
            result: result.clone(),
            reply: reply_tx,
        })
        .map_err(|_| "Wayland overlay worker stopped".to_string())?;
    let outcome = reply_rx
        .recv_timeout(COMMAND_TIMEOUT)
        .map_err(|_| "Wayland overlay timed out while mapping the layer surface".to_string())?;
    if outcome.is_ok() {
        client.active.store(true, Ordering::Release);
    }
    outcome
}

pub fn update(result: &RelicOverlayResult) -> Result<bool, String> {
    let mut guard = CLIENT.lock().unwrap_or_else(|error| error.into_inner());
    let Some(client) = guard.as_mut() else {
        return Ok(false);
    };
    if !client.active.load(Ordering::Acquire) {
        return Ok(false);
    }
    let (reply_tx, reply_rx) = mpsc::sync_channel(1);
    client
        .tx
        .send(Command::Update {
            result: result.clone(),
            reply: reply_tx,
        })
        .map_err(|_| "Wayland overlay worker stopped".to_string())?;
    reply_rx
        .recv_timeout(COMMAND_TIMEOUT)
        .map_err(|_| "Wayland overlay timed out while updating the layer surface".to_string())??;
    Ok(true)
}

pub fn hide() {
    let guard = CLIENT.lock().unwrap_or_else(|error| error.into_inner());
    if let Some(client) = guard.as_ref() {
        client.active.store(false, Ordering::Release);
        let _ = client.tx.send(Command::Hide);
    }
}

pub fn is_active() -> bool {
    CLIENT
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .as_ref()
        .is_some_and(|client| client.active.load(Ordering::Acquire))
}

fn ensure_client() -> Result<(), String> {
    let mut guard = CLIENT.lock().unwrap_or_else(|error| error.into_inner());
    if guard.is_none() {
        let (tx, rx) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let active = Arc::new(AtomicBool::new(false));
        let thread_active = active.clone();
        std::thread::Builder::new()
            .name("relic-wayland-overlay".into())
            .spawn(move || {
                let result = run_worker(rx, thread_active.clone(), &ready_tx);
                thread_active.store(false, Ordering::Release);
                if let Err(error) = result {
                    let _ = ready_tx.try_send(Err(error.clone()));
                    eprintln!("tennoworth: Wayland relic overlay stopped: {error}");
                }
            })
            .map_err(|error| format!("starting Wayland overlay worker: {error}"))?;
        ready_rx
            .recv_timeout(COMMAND_TIMEOUT)
            .map_err(|_| "Wayland overlay worker initialization timed out".to_string())??;
        *guard = Some(ClientHandle { tx, active });
    }
    Ok(())
}

#[derive(Debug, Clone)]
struct OutputInfo {
    output: WlOutput,
    name: String,
    logical_x: i32,
    logical_y: i32,
    logical_width: u32,
    logical_height: u32,
    mode_width: u32,
    mode_height: u32,
    scale: i32,
}

impl OutputInfo {
    fn ready(&self) -> bool {
        self.logical_width > 0 && self.logical_height > 0
    }
}

struct BufferSlot {
    buffer: WlBuffer,
    image: ImageSurface,
    busy: bool,
}

struct SurfaceState {
    surface: WlSurface,
    layer_surface: ZwlrLayerSurfaceV1,
    viewport: Option<WpViewport>,
    fractional_scale: Option<WpFractionalScaleV1>,
    logical_width: i32,
    logical_height: i32,
    scale_120: u32,
    configured: bool,
    result: RelicOverlayResult,
    buffers: Vec<BufferSlot>,
}

struct WaylandState {
    qh: QueueHandle<WaylandState>,
    compositor: WlCompositor,
    shm: WlShm,
    layer_shell: ZwlrLayerShellV1,
    viewporter: Option<WpViewporter>,
    fractional_scale_manager: Option<WpFractionalScaleManagerV1>,
    outputs: Vec<OutputInfo>,
    surface: Option<SurfaceState>,
}

fn run_worker(
    rx: mpsc::Receiver<Command>,
    active: Arc<AtomicBool>,
    ready_tx: &mpsc::SyncSender<Result<(), String>>,
) -> Result<(), String> {
    let connection = Connection::connect_to_env()
        .map_err(|error| format!("connecting to the Wayland compositor: {error}"))?;
    let (globals, mut event_queue) = registry_queue_init::<WaylandState>(&connection)
        .map_err(|error| format!("reading Wayland globals: {error}"))?;
    let qh = event_queue.handle();
    let compositor = globals
        .bind::<WlCompositor, _, _>(&qh, 4..=6, ())
        .map_err(|error| format!("Wayland compositor protocol unavailable: {error}"))?;
    let shm = globals
        .bind::<WlShm, _, _>(&qh, 1..=2, ())
        .map_err(|error| format!("Wayland shared-memory protocol unavailable: {error}"))?;
    let layer_shell = globals
        .bind::<ZwlrLayerShellV1, _, _>(&qh, 1..=5, ())
        .map_err(|error| format!("Wayland layer-shell protocol unavailable: {error}"))?;
    let viewporter = globals.bind::<WpViewporter, _, _>(&qh, 1..=1, ()).ok();
    let fractional_scale_manager = globals
        .bind::<WpFractionalScaleManagerV1, _, _>(&qh, 1..=1, ())
        .ok();
    let xdg_output_manager = globals
        .bind::<ZxdgOutputManagerV1, _, _>(&qh, 1..=3, ())
        .ok();

    let mut state = WaylandState {
        qh: qh.clone(),
        compositor,
        shm,
        layer_shell,
        viewporter,
        fractional_scale_manager,
        outputs: Vec::new(),
        surface: None,
    };
    for global in globals.contents().clone_list() {
        if global.interface == WlOutput::interface().name {
            let index = state.outputs.len();
            let output = globals.registry().bind::<WlOutput, _, _>(
                global.name,
                global.version.min(4),
                &qh,
                index,
            );
            state.outputs.push(OutputInfo {
                output: output.clone(),
                name: String::new(),
                logical_x: 0,
                logical_y: 0,
                logical_width: 0,
                logical_height: 0,
                mode_width: 0,
                mode_height: 0,
                scale: 1,
            });
            if let Some(manager) = xdg_output_manager.as_ref() {
                manager.get_xdg_output(&output, &qh, index);
            }
        }
    }
    event_queue
        .roundtrip(&mut state)
        .map_err(|error| format!("reading Wayland output geometry: {error}"))?;
    if state.outputs.iter().all(|output| !output.ready()) {
        let error = "Wayland compositor did not provide usable output geometry".to_string();
        let _ = ready_tx.send(Err(error.clone()));
        return Err(error);
    }
    let _ = ready_tx.send(Ok(()));

    loop {
        while let Ok(command) = rx.try_recv() {
            match command {
                Command::Show {
                    geometry,
                    x_output,
                    result,
                    reply,
                } => {
                    let outcome = map_surface(
                        &connection,
                        &mut event_queue,
                        &mut state,
                        geometry,
                        x_output.as_ref(),
                        result,
                    );
                    active.store(outcome.is_ok(), Ordering::Release);
                    let _ = reply.send(outcome);
                }
                Command::Update { result, reply } => {
                    let outcome = update_surface(&mut state, result);
                    if outcome.is_err() {
                        active.store(false, Ordering::Release);
                    }
                    let _ = reply.send(outcome);
                }
                Command::Hide => {
                    unmap_surface(&mut state);
                    active.store(false, Ordering::Release);
                }
            }
        }

        event_queue
            .dispatch_pending(&mut state)
            .map_err(|error| format!("dispatching Wayland overlay events: {error}"))?;
        connection
            .flush()
            .map_err(|error| format!("flushing Wayland overlay events: {error}"))?;
        if let Some(read_guard) = event_queue.prepare_read() {
            let mut fds = [PollFd::new(
                &connection,
                PollFlags::IN | PollFlags::ERR | PollFlags::HUP,
            )];
            let timeout = Timespec {
                tv_sec: 0,
                tv_nsec: 20_000_000,
            };
            if poll(&mut fds, Some(&timeout)).is_ok_and(|ready| ready > 0) {
                read_guard
                    .read()
                    .map_err(|error| format!("reading Wayland overlay events: {error}"))?;
            }
        }
    }
}

fn map_surface(
    connection: &Connection,
    event_queue: &mut EventQueue<WaylandState>,
    state: &mut WaylandState,
    geometry: OverlayGeometry,
    x_output: Option<&XOutput>,
    result: RelicOverlayResult,
) -> Result<(), String> {
    unmap_surface(state);
    let (output_index, local_x, local_y, logical_width, logical_height) =
        map_geometry_to_output(&state.outputs, geometry, x_output)?;
    let output = state
        .outputs
        .get(output_index)
        .ok_or_else(|| "selected Wayland output disappeared".to_string())?
        .clone();
    let qh = event_queue.handle();
    let surface = state.compositor.create_surface(&qh, ());
    let region: WlRegion = state.compositor.create_region(&qh, ());
    surface.set_input_region(Some(&region));
    region.destroy();
    let layer_surface = state.layer_shell.get_layer_surface(
        &surface,
        Some(&output.output),
        Layer::Overlay,
        "tennoworth-relic-overlay".into(),
        &qh,
        (),
    );
    layer_surface.set_anchor(Anchor::Top | Anchor::Left);
    layer_surface.set_exclusive_zone(-1);
    layer_surface.set_keyboard_interactivity(KeyboardInteractivity::None);
    layer_surface.set_margin(local_y, 0, 0, local_x);
    layer_surface.set_size(logical_width as u32, logical_height as u32);
    let viewport = state
        .viewporter
        .as_ref()
        .map(|viewporter| viewporter.get_viewport(&surface, &qh, ()));
    let fractional_scale = state
        .fractional_scale_manager
        .as_ref()
        .map(|manager| manager.get_fractional_scale(&surface, &qh, ()));
    state.surface = Some(SurfaceState {
        surface: surface.clone(),
        layer_surface,
        viewport,
        fractional_scale,
        logical_width,
        logical_height,
        scale_120: initial_scale_120(&output),
        configured: false,
        result,
        buffers: Vec::new(),
    });
    surface.commit();
    connection
        .flush()
        .map_err(|error| format!("requesting Wayland layer configuration: {error}"))?;
    for _ in 0..4 {
        event_queue
            .roundtrip(state)
            .map_err(|error| format!("configuring Wayland layer surface: {error}"))?;
        if state
            .surface
            .as_ref()
            .is_some_and(|surface| surface.configured)
        {
            break;
        }
    }
    if !state
        .surface
        .as_ref()
        .is_some_and(|surface| surface.configured)
    {
        unmap_surface(state);
        return Err("Wayland compositor did not configure the layer surface".into());
    }
    render_current_surface(state)
}

fn update_surface(state: &mut WaylandState, result: RelicOverlayResult) -> Result<(), String> {
    if let Some(surface) = state.surface.as_mut() {
        surface.result = result;
    } else {
        return Err("Wayland layer surface is not visible".into());
    }
    render_current_surface(state)
}

fn render_current_surface(state: &mut WaylandState) -> Result<(), String> {
    let qh = state.qh.clone();
    let surface = state
        .surface
        .as_mut()
        .ok_or_else(|| "Wayland layer surface is not visible".to_string())?;
    let scale = surface.scale_120.max(120) as f64 / 120.0;
    let buffer_width = (surface.logical_width as f64 * scale).ceil().max(1.0) as i32;
    let buffer_height = (surface.logical_height as f64 * scale).ceil().max(1.0) as i32;
    if surface.buffers.is_empty() {
        for index in 0..BUFFER_COUNT {
            surface.buffers.push(create_buffer(
                &state.shm,
                &qh,
                index,
                buffer_width,
                buffer_height,
            )?);
        }
    }
    let slot = surface
        .buffers
        .iter_mut()
        .find(|slot| !slot.busy)
        .ok_or_else(|| "Wayland overlay buffers are still in use".to_string())?;
    draw_overlay(
        &slot.image,
        &surface.result,
        surface.logical_width,
        surface.logical_height,
        scale,
    )?;
    slot.busy = true;
    if let Some(viewport) = surface.viewport.as_ref() {
        viewport.set_destination(surface.logical_width, surface.logical_height);
    }
    surface.surface.attach(Some(&slot.buffer), 0, 0);
    surface
        .surface
        .damage_buffer(0, 0, buffer_width, buffer_height);
    surface.surface.commit();
    Ok(())
}

fn create_buffer(
    shm: &WlShm,
    qh: &QueueHandle<WaylandState>,
    index: usize,
    width: i32,
    height: i32,
) -> Result<BufferSlot, String> {
    let stride = Format::ARgb32
        .stride_for_width(width as u32)
        .map_err(|error| format!("calculating Wayland overlay stride: {error}"))?;
    let size = i64::from(stride) * i64::from(height);
    let name = CString::new("tennoworth-relic-overlay")
        .map_err(|error| format!("creating Wayland shared-memory name: {error}"))?;
    let fd = memfd_create(name.as_c_str(), MemfdFlags::CLOEXEC)
        .map_err(|error| format!("creating Wayland shared-memory buffer: {error}"))?;
    ftruncate(&fd, size as u64)
        .map_err(|error| format!("sizing Wayland shared-memory buffer: {error}"))?;
    let file = File::from(fd);
    // The memfd has just been sized to the full pool and remains owned by this
    // process for the lifetime of the Cairo mapping.
    let mmap = unsafe { MmapMut::map_mut(&file) }
        .map_err(|error| format!("mapping Wayland shared-memory buffer: {error}"))?;
    let image = ImageSurface::create_for_data(mmap, Format::ARgb32, width, height, stride)
        .map_err(|error| format!("creating Cairo overlay surface: {error}"))?;
    let pool: WlShmPool = shm.create_pool(file.as_fd(), size as i32, qh, ());
    let buffer = pool.create_buffer(
        0,
        width,
        height,
        stride,
        wl_shm::Format::Argb8888,
        qh,
        index,
    );
    pool.destroy();
    Ok(BufferSlot {
        buffer,
        image,
        busy: false,
    })
}

fn unmap_surface(state: &mut WaylandState) {
    if let Some(surface) = state.surface.take() {
        surface.surface.attach(None, 0, 0);
        surface.surface.commit();
        for slot in surface.buffers {
            slot.buffer.destroy();
        }
        if let Some(fractional_scale) = surface.fractional_scale {
            fractional_scale.destroy();
        }
        if let Some(viewport) = surface.viewport {
            viewport.destroy();
        }
        surface.layer_surface.destroy();
        surface.surface.destroy();
    }
}

fn initial_scale_120(output: &OutputInfo) -> u32 {
    if output.logical_width > 0 && output.mode_width > 0 {
        ((output.mode_width as f64 / output.logical_width as f64) * 120.0)
            .round()
            .max(120.0) as u32
    } else {
        output.scale.max(1) as u32 * 120
    }
}

fn map_geometry_to_output(
    outputs: &[OutputInfo],
    geometry: OverlayGeometry,
    x_output: Option<&XOutput>,
) -> Result<(usize, i32, i32, i32, i32), String> {
    let index = x_output
        .and_then(|x_output| {
            outputs
                .iter()
                .position(|output| output.ready() && output.name == x_output.name)
        })
        .or_else(|| {
            let center_x = geometry.x + geometry.width as i32 / 2;
            let center_y = geometry.y + geometry.height as i32 / 2;
            outputs.iter().position(|output| {
                output.ready()
                    && center_x >= output.logical_x
                    && center_x < output.logical_x + output.logical_width as i32
                    && center_y >= output.logical_y
                    && center_y < output.logical_y + output.logical_height as i32
            })
        })
        .or_else(|| outputs.iter().position(OutputInfo::ready))
        .ok_or_else(|| "no usable Wayland output is available".to_string())?;
    let output = outputs
        .get(index)
        .ok_or_else(|| "selected Wayland output is unavailable".to_string())?;
    let (mut x, mut y, mut width, mut height) = logical_geometry(
        geometry,
        x_output.filter(|x_output| x_output.name == output.name),
        output.logical_x,
        output.logical_y,
        output.logical_width,
        output.logical_height,
    );
    width = width.clamp(1, output.logical_width as i32);
    height = height.clamp(1, output.logical_height as i32);
    x = x.clamp(0, output.logical_width as i32 - width);
    y = y.clamp(0, output.logical_height as i32 - height);
    Ok((index, x, y, width, height))
}

fn logical_geometry(
    geometry: OverlayGeometry,
    x_output: Option<&XOutput>,
    logical_x: i32,
    logical_y: i32,
    logical_width: u32,
    logical_height: u32,
) -> (i32, i32, i32, i32) {
    if let Some(x_output) = x_output.filter(|output| output.width > 0 && output.height > 0) {
        let sx = logical_width as f64 / x_output.width as f64;
        let sy = logical_height as f64 / x_output.height as f64;
        return (
            ((geometry.x - x_output.x) as f64 * sx).round() as i32,
            ((geometry.y - x_output.y) as f64 * sy).round() as i32,
            (geometry.width as f64 * sx).round().max(1.0) as i32,
            (geometry.height as f64 * sy).round().max(1.0) as i32,
        );
    }
    (
        geometry.x - logical_x,
        geometry.y - logical_y,
        geometry.width as i32,
        geometry.height as i32,
    )
}

fn x_output_for_geometry(geometry: OverlayGeometry) -> Option<XOutput> {
    use xcb::{randr, Connection as XConnection, Xid};

    let (connection, screen_index) = XConnection::connect(None).ok()?;
    let screen = connection.get_setup().roots().nth(screen_index as usize)?;
    let resources = connection
        .wait_for_reply(connection.send_request(&randr::GetScreenResourcesCurrent {
            window: screen.root(),
        }))
        .ok()?;
    let center_x = geometry.x + geometry.width as i32 / 2;
    let center_y = geometry.y + geometry.height as i32 / 2;
    let mut candidates = Vec::new();
    for output in resources.outputs() {
        let info = match connection.wait_for_reply(connection.send_request(&randr::GetOutputInfo {
            output: *output,
            config_timestamp: resources.config_timestamp(),
        })) {
            Ok(info) => info,
            Err(_) => continue,
        };
        let crtc = info.crtc();
        if crtc.resource_id() == 0 {
            continue;
        }
        let crtc_info =
            match connection.wait_for_reply(connection.send_request(&randr::GetCrtcInfo {
                crtc,
                config_timestamp: resources.config_timestamp(),
            })) {
                Ok(info) => info,
                Err(_) => continue,
            };
        let candidate = XOutput {
            name: String::from_utf8_lossy(info.name()).into_owned(),
            x: i32::from(crtc_info.x()),
            y: i32::from(crtc_info.y()),
            width: u32::from(crtc_info.width()),
            height: u32::from(crtc_info.height()),
        };
        if candidate.width > 0 && candidate.height > 0 {
            candidates.push(candidate);
        }
    }
    candidates
        .iter()
        .find(|output| {
            center_x >= output.x
                && center_x < output.x + output.width as i32
                && center_y >= output.y
                && center_y < output.y + output.height as i32
        })
        .cloned()
        .or_else(|| {
            candidates.into_iter().max_by_key(|output| {
                intersection_area(
                    geometry,
                    OverlayGeometry {
                        x: output.x,
                        y: output.y,
                        width: output.width,
                        height: output.height,
                    },
                )
            })
        })
}

fn intersection_area(a: OverlayGeometry, b: OverlayGeometry) -> u64 {
    let left = a.x.max(b.x);
    let top = a.y.max(b.y);
    let right = (a.x + a.width as i32).min(b.x + b.width as i32);
    let bottom = (a.y + a.height as i32).min(b.y + b.height as i32);
    u64::from((right - left).max(0) as u32) * u64::from((bottom - top).max(0) as u32)
}

fn draw_overlay(
    image: &ImageSurface,
    result: &RelicOverlayResult,
    logical_width: i32,
    logical_height: i32,
    buffer_scale: f64,
) -> Result<(), String> {
    let context =
        Context::new(image).map_err(|error| format!("creating Cairo overlay context: {error}"))?;
    context.scale(buffer_scale, buffer_scale);
    context.set_operator(Operator::Source);
    context.set_source_rgba(0.0, 0.0, 0.0, 0.0);
    context
        .paint()
        .map_err(|error| format!("clearing Cairo overlay surface: {error}"))?;
    context.set_operator(Operator::Over);
    for slot in &result.slots {
        let box_x = slot.box_.x * logical_width as f64;
        let box_y = slot.box_.y * logical_height as f64 + 8.0;
        let box_width = slot.box_.width * logical_width as f64;
        let card_width = (250.0 * result.scale).min((box_width - 16.0).max(80.0));
        let card_height = (if slot.confidence < 0.9 { 78.0 } else { 66.0 }) * result.scale;
        let x = box_x + (box_width - card_width) / 2.0;
        let y = box_y;
        let best_color = if slot.best_platinum {
            (0.902, 0.722, 0.361)
        } else if slot.confidence < 0.9 {
            (0.933, 0.561, 0.439)
        } else {
            (0.4, 0.502, 0.561)
        };
        rounded_rectangle(&context, x, y, card_width, card_height, 7.0);
        context.set_source_rgba(0.035, 0.067, 0.09, 0.91);
        context.fill_preserve().map_err(cairo_error)?;
        context.set_source_rgba(best_color.0, best_color.1, best_color.2, 1.0);
        context.set_line_width(1.0);
        context.stroke().map_err(cairo_error)?;

        if slot.best_platinum || (slot.best_ducats && !slot.best_platinum) {
            let label = if slot.best_platinum {
                "BEST PLAT"
            } else {
                "BEST DUCATS"
            };
            let flag_color = if slot.best_platinum {
                (0.902, 0.722, 0.361)
            } else {
                (0.459, 0.796, 0.816)
            };
            rounded_rectangle(&context, x + card_width - 62.0, y - 9.0, 58.0, 17.0, 4.0);
            context.set_source_rgb(flag_color.0, flag_color.1, flag_color.2);
            context.fill().map_err(cairo_error)?;
            draw_text(
                &context,
                label,
                x + card_width - 58.0,
                y + 2.0,
                9.0,
                FontWeight::Bold,
                52.0,
                (0.03, 0.06, 0.07),
                true,
            )?;
        }

        let name = slot.name.as_deref().unwrap_or(&slot.raw_text);
        draw_text(
            &context,
            name,
            x + 11.0,
            y + 19.0 * result.scale,
            12.0 * result.scale,
            FontWeight::Bold,
            card_width - 66.0,
            (0.929, 0.949, 0.957),
            false,
        )?;
        let price = slot.live_platinum.or(slot.cached_platinum);
        draw_text(
            &context,
            &price.map_or_else(|| "-".into(), |value| format!("{value}p")),
            x + 11.0,
            y + 47.0 * result.scale,
            22.0 * result.scale,
            FontWeight::Bold,
            56.0,
            (0.902, 0.722, 0.361),
            true,
        )?;
        let facts = format!(
            "{}   {}   {}",
            slot.ducats
                .map_or_else(|| "-d".into(), |value| format!("{value}d")),
            slot.owned
                .map_or_else(|| "own -".into(), |value| format!("own {value}")),
            if slot.live_platinum.is_some() {
                "live"
            } else {
                "cached"
            }
        );
        draw_text(
            &context,
            &facts,
            x + 70.0,
            y + 43.0 * result.scale,
            10.0 * result.scale,
            FontWeight::Normal,
            card_width - 80.0,
            (0.714, 0.757, 0.784),
            true,
        )?;
        if slot.confidence < 0.9 {
            draw_text(
                &context,
                &format!("check name · {:.0}%", slot.confidence * 100.0),
                x + 11.0,
                y + 64.0 * result.scale,
                9.0 * result.scale,
                FontWeight::Normal,
                card_width - 22.0,
                (0.933, 0.561, 0.439),
                true,
            )?;
        }
    }
    image.flush();
    Ok(())
}

#[allow(
    clippy::too_many_arguments,
    reason = "text drawing parameters are the renderer's compact style contract"
)]
fn draw_text(
    context: &Context,
    text: &str,
    x: f64,
    y: f64,
    size: f64,
    weight: FontWeight,
    max_width: f64,
    color: (f64, f64, f64),
    monospace: bool,
) -> Result<(), String> {
    context.select_font_face(
        if monospace { "Monospace" } else { "Sans" },
        FontSlant::Normal,
        weight,
    );
    context.set_font_size(size);
    context.set_source_rgb(color.0, color.1, color.2);
    let fitted = ellipsize(context, text, max_width)?;
    context.move_to(x, y);
    context.show_text(&fitted).map_err(cairo_error)
}

fn ellipsize(context: &Context, text: &str, max_width: f64) -> Result<String, String> {
    if context.text_extents(text).map_err(cairo_error)?.width() <= max_width {
        return Ok(text.into());
    }
    let mut fitted = text.to_string();
    while !fitted.is_empty() {
        fitted.pop();
        let candidate = format!("{}…", fitted.trim_end());
        if context
            .text_extents(&candidate)
            .map_err(cairo_error)?
            .width()
            <= max_width
        {
            return Ok(candidate);
        }
    }
    Ok("…".into())
}

fn rounded_rectangle(context: &Context, x: f64, y: f64, width: f64, height: f64, radius: f64) {
    let radius = radius.min(width / 2.0).min(height / 2.0);
    context.new_sub_path();
    context.arc(
        x + width - radius,
        y + radius,
        radius,
        -std::f64::consts::FRAC_PI_2,
        0.0,
    );
    context.arc(
        x + width - radius,
        y + height - radius,
        radius,
        0.0,
        std::f64::consts::FRAC_PI_2,
    );
    context.arc(
        x + radius,
        y + height - radius,
        radius,
        std::f64::consts::FRAC_PI_2,
        std::f64::consts::PI,
    );
    context.arc(
        x + radius,
        y + radius,
        radius,
        std::f64::consts::PI,
        std::f64::consts::PI * 1.5,
    );
    context.close_path();
}

fn cairo_error(error: cairo::Error) -> String {
    format!("rendering native Wayland overlay: {error}")
}

impl Dispatch<WlRegistry, GlobalListContents> for WaylandState {
    fn event(
        _: &mut Self,
        _: &WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WlOutput, usize> for WaylandState {
    fn event(
        state: &mut Self,
        _: &WlOutput,
        event: wl_output::Event,
        index: &usize,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some(output) = state.outputs.get_mut(*index) else {
            return;
        };
        match event {
            wl_output::Event::Name { name } => output.name = name,
            wl_output::Event::Mode {
                flags: WEnum::Value(flags),
                width,
                height,
                ..
            } if flags.contains(wl_output::Mode::Current) && width > 0 && height > 0 => {
                output.mode_width = width as u32;
                output.mode_height = height as u32;
            }
            wl_output::Event::Scale { factor } => output.scale = factor.max(1),
            _ => {}
        }
    }
}

impl Dispatch<ZxdgOutputV1, usize> for WaylandState {
    fn event(
        state: &mut Self,
        _: &ZxdgOutputV1,
        event: zxdg_output_v1::Event,
        index: &usize,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some(output) = state.outputs.get_mut(*index) else {
            return;
        };
        match event {
            zxdg_output_v1::Event::LogicalPosition { x, y } => {
                output.logical_x = x;
                output.logical_y = y;
            }
            zxdg_output_v1::Event::LogicalSize { width, height } if width > 0 && height > 0 => {
                output.logical_width = width as u32;
                output.logical_height = height as u32;
            }
            zxdg_output_v1::Event::Name { name } if output.name.is_empty() => output.name = name,
            _ => {}
        }
    }
}

impl Dispatch<ZwlrLayerSurfaceV1, ()> for WaylandState {
    fn event(
        state: &mut Self,
        layer_surface: &ZwlrLayerSurfaceV1,
        event: zwlr_layer_surface_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_layer_surface_v1::Event::Configure { serial, .. } => {
                layer_surface.ack_configure(serial);
                if let Some(surface) = state.surface.as_mut() {
                    surface.configured = true;
                }
            }
            zwlr_layer_surface_v1::Event::Closed => unmap_surface(state),
            _ => {}
        }
    }
}

impl Dispatch<WpFractionalScaleV1, ()> for WaylandState {
    fn event(
        state: &mut Self,
        _: &WpFractionalScaleV1,
        event: wp_fractional_scale_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wp_fractional_scale_v1::Event::PreferredScale { scale } = event {
            if let Some(surface) = state.surface.as_mut() {
                surface.scale_120 = scale.max(120);
            }
        }
    }
}

impl Dispatch<WlBuffer, usize> for WaylandState {
    fn event(
        state: &mut Self,
        _: &WlBuffer,
        event: wl_buffer::Event,
        index: &usize,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if matches!(event, wl_buffer::Event::Release) {
            if let Some(slot) = state
                .surface
                .as_mut()
                .and_then(|surface| surface.buffers.get_mut(*index))
            {
                slot.busy = false;
            }
        }
    }
}

delegate_noop!(WaylandState: ignore WlCompositor);
delegate_noop!(WaylandState: ignore WlShm);
delegate_noop!(WaylandState: ignore WlShmPool);
delegate_noop!(WaylandState: ignore WlSurface);
delegate_noop!(WaylandState: ignore WlRegion);
delegate_noop!(WaylandState: ignore ZwlrLayerShellV1);
delegate_noop!(WaylandState: ignore ZxdgOutputManagerV1);
delegate_noop!(WaylandState: ignore WpViewporter);
delegate_noop!(WaylandState: ignore WpViewport);
delegate_noop!(WaylandState: ignore WpFractionalScaleManagerV1);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intersection_uses_only_the_overlapping_area() {
        assert_eq!(
            intersection_area(
                OverlayGeometry {
                    x: 10,
                    y: 10,
                    width: 20,
                    height: 20
                },
                OverlayGeometry {
                    x: 20,
                    y: 0,
                    width: 20,
                    height: 20
                },
            ),
            100
        );
        assert_eq!(
            intersection_area(
                OverlayGeometry {
                    x: 0,
                    y: 0,
                    width: 10,
                    height: 10
                },
                OverlayGeometry {
                    x: 20,
                    y: 20,
                    width: 10,
                    height: 10
                },
            ),
            0
        );
    }

    #[test]
    fn xwayland_pixels_map_to_fractional_wayland_coordinates() {
        let x_output = XOutput {
            name: "DP-3".into(),
            x: 2560,
            y: 0,
            width: 2560,
            height: 1440,
        };
        assert_eq!(
            logical_geometry(
                OverlayGeometry {
                    x: 3200,
                    y: 360,
                    width: 1280,
                    height: 180,
                },
                Some(&x_output),
                1707,
                0,
                1707,
                960,
            ),
            (427, 240, 854, 120)
        );
    }

    #[test]
    fn native_renderer_draws_the_shared_overlay_fixture_on_transparency() {
        let result: RelicOverlayResult = serde_json::from_str(include_str!(
            "../../../tests/fixtures/relic-ocr/result.json"
        ))
        .expect("shared overlay result fixture should deserialize");
        let mut image = ImageSurface::create(Format::ARgb32, 1000, 150)
            .expect("test image surface should be created");
        draw_overlay(&image, &result, 1000, 150, 1.0)
            .expect("shared overlay fixture should render");
        let pixels = image.data().expect("rendered pixels should be readable");
        assert!(pixels.iter().any(|channel| *channel != 0));
        assert_eq!(&pixels[..4], &[0, 0, 0, 0]);
    }
}
