use super::{is_game_window, GameFocus, GameWindow};
use windows::Win32::Foundation::RECT;
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowRect, GetWindowTextW, GetWindowThreadProcessId,
};

pub(crate) fn game_focus() -> GameFocus {
    // SAFETY: no arguments; returns a null handle when no window is in front.
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.is_invalid() {
        return GameFocus::Background;
    }
    let mut pid = 0u32;
    // SAFETY: hwnd came from GetForegroundWindow and `pid` outlives the call.
    unsafe { GetWindowThreadProcessId(hwnd, Some(&raw mut pid)) };
    let mut buffer = [0u16; 256];
    // SAFETY: the call writes at most buffer.len() UTF-16 units.
    let length = unsafe { GetWindowTextW(hwnd, &mut buffer) };
    let title = buffer
        .get(..usize::try_from(length).unwrap_or(0))
        .map(String::from_utf16_lossy)
        .unwrap_or_default();
    let pid = (pid != 0).then_some(pid);
    if !is_game_window(pid, &title, game_acquisition::scan::find_wf_pid()) {
        return GameFocus::Background;
    }
    let mut rect = RECT::default();
    // SAFETY: hwnd is a live top-level window handle and rect is writable.
    if unsafe { GetWindowRect(hwnd, &mut rect) }.is_err() {
        return GameFocus::Unknown;
    }
    let (Ok(width), Ok(height)) = (
        u32::try_from(rect.right.saturating_sub(rect.left)),
        u32::try_from(rect.bottom.saturating_sub(rect.top)),
    ) else {
        return GameFocus::Unknown;
    };
    GameFocus::Focused(GameWindow {
        x: rect.left,
        y: rect.top,
        width,
        height,
    })
}
